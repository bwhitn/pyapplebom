use apple_bom::{
    format::{BomBlock, ParsedBom},
    BomPath, BomPathType,
};
use pyo3::{
    create_exception,
    exceptions::{PyException, PyOSError, PyTypeError},
    prelude::*,
    types::{PyDict, PyList},
    wrap_pyfunction, Bound,
};
use std::{
    any::Any,
    panic::{catch_unwind, AssertUnwindSafe},
};

create_exception!(pyapplebom, BomParseError, PyException);

fn bom_error_to_py(err: apple_bom::Error) -> PyErr {
    BomParseError::new_err(err.to_string())
}

fn panic_payload_to_string(payload: Box<dyn Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "unknown panic payload".to_string()
    }
}

enum SafeBomCall<T> {
    Value(T),
    MissingVariable,
    Error(String),
}

fn safe_bom_call<T, F>(func: F) -> SafeBomCall<T>
where
    F: FnOnce() -> Result<T, apple_bom::Error>,
{
    match catch_unwind(AssertUnwindSafe(func)) {
        Ok(Ok(value)) => SafeBomCall::Value(value),
        Ok(Err(apple_bom::Error::NoVar(_))) => SafeBomCall::MissingVariable,
        Ok(Err(err)) => SafeBomCall::Error(err.to_string()),
        Err(payload) => SafeBomCall::Error(format!(
            "apple-bom parser panicked: {}",
            panic_payload_to_string(payload)
        )),
    }
}

fn path_type_name(path_type: BomPathType) -> &'static str {
    match path_type {
        BomPathType::File => "file",
        BomPathType::Directory => "directory",
        BomPathType::Link => "link",
        BomPathType::Dev => "device",
        BomPathType::Other(_) => "other",
    }
}

fn path_to_dict<'py>(py: Python<'py>, path: &BomPath) -> PyResult<Bound<'py, PyDict>> {
    let item = PyDict::new_bound(py);
    let path_type = path.path_type();
    let path_type_raw: u8 = path_type.into();

    item.set_item("path", path.path())?;
    item.set_item("path_type", path_type_name(path_type))?;
    item.set_item("path_type_raw", path_type_raw)?;
    item.set_item("file_mode", path.file_mode())?;
    item.set_item("symbolic_mode", path.symbolic_mode())?;
    item.set_item("user_id", path.user_id())?;
    item.set_item("group_id", path.group_id())?;
    item.set_item("mtime", path.modified_time().timestamp())?;
    item.set_item("mtime_iso8601", path.modified_time().to_rfc3339())?;
    item.set_item("size", path.size())?;
    item.set_item("crc32", path.crc32())?;
    item.set_item("link_name", path.link_name())?;

    Ok(item)
}

fn path_record_fields<'py>(
    item: &Bound<'py, PyDict>,
    record: &apple_bom::format::BomBlockPathRecord<'_>,
) -> PyResult<()> {
    let path_type = BomPathType::from(record.path_type);

    item.set_item("path_type", path_type_name(path_type))?;
    item.set_item("path_type_raw", record.path_type)?;
    item.set_item("a", record.a)?;
    item.set_item("architecture", record.architecture)?;
    item.set_item("mode", record.mode)?;
    item.set_item("user", record.user)?;
    item.set_item("group", record.group)?;
    item.set_item("mtime", record.mtime)?;
    item.set_item("size", record.size)?;
    item.set_item("b", record.b)?;
    item.set_item("checksum_or_type", record.checksum_or_type)?;
    item.set_item("link_name_length", record.link_name_length)?;
    item.set_item("link_name", record.string_link_name())?;

    Ok(())
}

fn serialize_path_list<'py>(py: Python<'py>, paths: &[BomPath]) -> PyResult<Bound<'py, PyList>> {
    let list = PyList::empty_bound(py);

    for path in paths {
        list.append(path_to_dict(py, path)?)?;
    }

    Ok(list)
}

fn append_block_entry<'py>(
    py: Python<'py>,
    bom: &ParsedBom<'_>,
    index: usize,
    include_raw_block_bytes: bool,
    blocks_list: &Bound<'py, PyList>,
) -> PyResult<()> {
    let entry = bom.blocks.blocks.get(index).ok_or_else(|| {
        PyTypeError::new_err(format!(
            "block index {index} out of range while serializing"
        ))
    })?;

    let block_dict = PyDict::new_bound(py);
    block_dict.set_item("index", index)?;
    block_dict.set_item("file_offset", entry.file_offset)?;
    block_dict.set_item("length", entry.length)?;

    let raw_data = bom.block_data(index).map_err(bom_error_to_py)?;

    if include_raw_block_bytes {
        block_dict.set_item("raw_hex", hex::encode(raw_data))?;
    }

    if raw_data.is_empty() {
        block_dict.set_item("kind", "Empty")?;
        blocks_list.append(block_dict)?;
        return Ok(());
    }

    // apple-bom's block type detector assumes at least 4 bytes for tree checks.
    if raw_data.len() < 4 {
        block_dict.set_item("kind", "Unknown")?;
        block_dict.set_item("parse_error", "block too small for type detection")?;
        blocks_list.append(block_dict)?;
        return Ok(());
    }

    match catch_unwind(AssertUnwindSafe(|| BomBlock::try_parse(bom, index))) {
        Err(payload) => {
            block_dict.set_item("kind", "Unknown")?;
            block_dict.set_item(
                "parse_error",
                format!(
                    "block parser panicked: {}",
                    panic_payload_to_string(payload)
                ),
            )?;
        }
        Ok(Err(err)) => {
            block_dict.set_item("kind", "Unknown")?;
            block_dict.set_item("parse_error", err.to_string())?;
        }
        Ok(Ok(BomBlock::Empty)) => {
            block_dict.set_item("kind", "Empty")?;
        }
        Ok(Ok(BomBlock::BomInfo(info))) => {
            block_dict.set_item("kind", "BomInfo")?;
            block_dict.set_item("version", info.version)?;
            block_dict.set_item("number_of_paths", info.number_of_paths)?;
            block_dict.set_item("number_of_info_entries", info.number_of_info_entries)?;

            let entries = PyList::empty_bound(py);
            for info_entry in &info.entries {
                let item = PyDict::new_bound(py);
                item.set_item("a", info_entry.a)?;
                item.set_item("b", info_entry.b)?;
                item.set_item("c", info_entry.c)?;
                item.set_item("d", info_entry.d)?;
                entries.append(item)?;
            }
            block_dict.set_item("entries", entries)?;
        }
        Ok(Ok(BomBlock::File(file))) => {
            block_dict.set_item("kind", "File")?;
            block_dict.set_item("parent_path_id", file.parent_path_id)?;
            block_dict.set_item("name", file.string_file_name())?;
        }
        Ok(Ok(BomBlock::PathInfoIndex(path_info))) => {
            block_dict.set_item("kind", "PathInfoIndex")?;
            block_dict.set_item("path_id", path_info.path_id)?;
            block_dict.set_item("path_record_index", path_info.path_record_index)?;
        }
        Ok(Ok(BomBlock::PathRecord(record))) => {
            block_dict.set_item("kind", "PathRecord")?;
            path_record_fields(&block_dict, &record)?;
        }
        Ok(Ok(BomBlock::PathRecordPointer(pointer))) => {
            block_dict.set_item("kind", "PathRecordPointer")?;
            block_dict.set_item("block_path_record_index", pointer.block_path_record_index)?;
        }
        Ok(Ok(BomBlock::Paths(paths))) => {
            block_dict.set_item("kind", "Paths")?;
            block_dict.set_item("is_path_info", paths.is_path_info)?;
            block_dict.set_item("count", paths.count)?;
            block_dict.set_item("next_paths_block_index", paths.next_paths_block_index)?;
            block_dict.set_item(
                "previous_paths_block_index",
                paths.previous_paths_block_index,
            )?;

            let path_entries = PyList::empty_bound(py);
            for path in &paths.paths {
                let item = PyDict::new_bound(py);
                item.set_item("block_index", path.block_index)?;
                item.set_item("file_index", path.file_index)?;
                path_entries.append(item)?;
            }

            block_dict.set_item("paths", path_entries)?;
        }
        Ok(Ok(BomBlock::Tree(tree))) => {
            block_dict.set_item("kind", "Tree")?;
            block_dict.set_item("tree", String::from_utf8_lossy(&tree.tree).to_string())?;
            block_dict.set_item("version", tree.version)?;
            block_dict.set_item("block_paths_index", tree.block_paths_index)?;
            block_dict.set_item("block_size", tree.block_size)?;
            block_dict.set_item("path_count", tree.path_count)?;
            block_dict.set_item("a", tree.a)?;
        }
        Ok(Ok(BomBlock::TreePointer(pointer))) => {
            block_dict.set_item("kind", "TreePointer")?;
            block_dict.set_item("block_tree_index", pointer.block_tree_index)?;
        }
        Ok(Ok(BomBlock::VIndex(vindex))) => {
            block_dict.set_item("kind", "VIndex")?;
            block_dict.set_item("a", vindex.a)?;
            block_dict.set_item("tree_block_index", vindex.tree_block_index)?;
            block_dict.set_item("b", vindex.b)?;
            block_dict.set_item("c", vindex.c)?;
        }
    }

    blocks_list.append(block_dict)?;

    Ok(())
}

fn parse_optional_path_section<'py>(
    py: Python<'py>,
    doc: &Bound<'py, PyDict>,
    parse_errors: &Bound<'py, PyDict>,
    name: &str,
    parser: impl FnOnce() -> Result<Vec<BomPath>, apple_bom::Error>,
) -> PyResult<()> {
    match safe_bom_call(parser) {
        SafeBomCall::Value(paths) => {
            doc.set_item(name, serialize_path_list(py, &paths)?)?;
        }
        SafeBomCall::MissingVariable => {
            doc.set_item(name, py.None())?;
        }
        SafeBomCall::Error(err) => {
            doc.set_item(name, py.None())?;
            parse_errors.set_item(name, err)?;
        }
    }

    Ok(())
}

fn parse_bom_document<'py>(
    py: Python<'py>,
    data: &[u8],
    source_path: Option<&str>,
    include_blocks: bool,
    include_raw_block_bytes: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let bom = ParsedBom::parse(data).map_err(bom_error_to_py)?;
    let doc = PyDict::new_bound(py);
    let parse_errors = PyDict::new_bound(py);

    doc.set_item("format", "apple-bom")?;
    doc.set_item("byte_length", data.len())?;

    if let Some(path) = source_path {
        doc.set_item("source_path", path)?;
    }

    let header = PyDict::new_bound(py);
    header.set_item(
        "magic",
        String::from_utf8_lossy(&bom.header.magic).to_string(),
    )?;
    header.set_item("version", bom.header.version)?;
    header.set_item("number_of_blocks", bom.header.number_of_blocks)?;
    header.set_item("blocks_index_offset", bom.header.blocks_index_offset)?;
    header.set_item("blocks_index_length", bom.header.blocks_index_length)?;
    header.set_item("vars_index_offset", bom.header.vars_index_offset)?;
    header.set_item("vars_index_length", bom.header.vars_index_length)?;
    doc.set_item("header", header)?;

    let blocks_index = PyDict::new_bound(py);
    blocks_index.set_item("count", bom.blocks.count)?;
    let block_entries = PyList::empty_bound(py);
    for (index, entry) in bom.blocks.blocks.iter().enumerate() {
        let item = PyDict::new_bound(py);
        item.set_item("index", index)?;
        item.set_item("file_offset", entry.file_offset)?;
        item.set_item("length", entry.length)?;
        block_entries.append(item)?;
    }
    blocks_index.set_item("entries", block_entries)?;
    doc.set_item("blocks_index", blocks_index)?;

    let variables = PyList::empty_bound(py);
    for var in &bom.vars.vars {
        let item = PyDict::new_bound(py);
        item.set_item("name", &var.name)?;
        item.set_item("name_length", var.name_length)?;
        item.set_item("block_index", var.block_index)?;
        variables.append(item)?;
    }
    doc.set_item("variables", variables)?;

    match safe_bom_call(|| bom.bom_info()) {
        SafeBomCall::Value(info) => {
            let info_dict = PyDict::new_bound(py);
            info_dict.set_item("version", info.version)?;
            info_dict.set_item("number_of_paths", info.number_of_paths)?;
            info_dict.set_item("number_of_info_entries", info.number_of_info_entries)?;

            let entries = PyList::empty_bound(py);
            for info_entry in &info.entries {
                let item = PyDict::new_bound(py);
                item.set_item("a", info_entry.a)?;
                item.set_item("b", info_entry.b)?;
                item.set_item("c", info_entry.c)?;
                item.set_item("d", info_entry.d)?;
                entries.append(item)?;
            }
            info_dict.set_item("entries", entries)?;

            doc.set_item("bom_info", info_dict)?;
        }
        SafeBomCall::MissingVariable => {
            doc.set_item("bom_info", py.None())?;
        }
        SafeBomCall::Error(err) => {
            doc.set_item("bom_info", py.None())?;
            parse_errors.set_item("bom_info", err)?;
        }
    }

    parse_optional_path_section(py, &doc, &parse_errors, "paths", || bom.paths())?;
    parse_optional_path_section(py, &doc, &parse_errors, "hl_index", || bom.hl_index())?;
    parse_optional_path_section(py, &doc, &parse_errors, "size64", || bom.size64())?;
    parse_optional_path_section(py, &doc, &parse_errors, "vindex", || bom.vindex())?;

    if include_blocks {
        let blocks = PyList::empty_bound(py);
        for index in 0..bom.blocks.blocks.len() {
            append_block_entry(py, &bom, index, include_raw_block_bytes, &blocks)?;
        }
        doc.set_item("blocks", blocks)?;
    } else {
        doc.set_item("blocks", py.None())?;
    }

    if parse_errors.len() == 0 {
        doc.set_item("parse_errors", py.None())?;
    } else {
        doc.set_item("parse_errors", parse_errors)?;
    }

    Ok(doc)
}

#[pyfunction(signature = (data, *, include_blocks = true, include_raw_block_bytes = false))]
fn parse_bom_bytes(
    py: Python<'_>,
    data: &[u8],
    include_blocks: bool,
    include_raw_block_bytes: bool,
) -> PyResult<PyObject> {
    let doc = parse_bom_document(py, data, None, include_blocks, include_raw_block_bytes)?;
    Ok(doc.into_py(py))
}

#[pyfunction(signature = (path, *, include_blocks = true, include_raw_block_bytes = false))]
fn parse_bom_file(
    py: Python<'_>,
    path: &str,
    include_blocks: bool,
    include_raw_block_bytes: bool,
) -> PyResult<PyObject> {
    let data = std::fs::read(path)
        .map_err(|err| PyOSError::new_err(format!("failed reading {path}: {err}")))?;

    let doc = parse_bom_document(
        py,
        &data,
        Some(path),
        include_blocks,
        include_raw_block_bytes,
    )?;

    Ok(doc.into_py(py))
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("BomParseError", m.py().get_type_bound::<BomParseError>())?;
    m.add_function(wrap_pyfunction!(parse_bom_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(parse_bom_file, m)?)?;

    Ok(())
}
