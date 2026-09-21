mod validation;

use apple_bom::{
    format::{BomBlock, ParsedBom},
    BomPath, BomPathType,
};
use pyo3::{
    create_exception,
    exceptions::{PyException, PyOSError, PyTypeError, PyValueError},
    intern,
    prelude::*,
    types::{PyDict, PyList, PyString},
    wrap_pyfunction, Bound,
};
use std::{
    any::Any,
    collections::HashMap,
    fs::File,
    io::Read,
    panic::{catch_unwind, AssertUnwindSafe},
};
use validation::{ParseLimits, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_PATHS};

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

enum ParsedPathTree {
    Value(Vec<BomPath>),
    Error(String),
}

struct PathTreeCache {
    parsed: HashMap<usize, ParsedPathTree>,
    remaining_uses: HashMap<usize, usize>,
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

fn path_type_name<'py>(py: Python<'py>, path_type: BomPathType) -> &'py Bound<'py, PyString> {
    match path_type {
        BomPathType::File => intern!(py, "file"),
        BomPathType::Directory => intern!(py, "directory"),
        BomPathType::Link => intern!(py, "link"),
        BomPathType::Dev => intern!(py, "device"),
        BomPathType::Other(_) => intern!(py, "other"),
    }
}

fn symbolic_mode(path_type: BomPathType, mode: u16) -> [u8; 10] {
    let mut result = [b'-'; 10];
    result[0] = match path_type {
        BomPathType::Directory => b'd',
        BomPathType::File => b'-',
        BomPathType::Link => b'l',
        BomPathType::Dev | BomPathType::Other(_) => b'?',
    };

    for (index, (mask, enabled)) in [
        (0o400, b'r'),
        (0o200, b'w'),
        (0o100, b'x'),
        (0o040, b'r'),
        (0o020, b'w'),
        (0o010, b'x'),
        (0o004, b'r'),
        (0o002, b'w'),
        (0o001, b'x'),
    ]
    .into_iter()
    .enumerate()
    {
        if mode & mask != 0 {
            result[index + 1] = enabled;
        }
    }

    result
}

fn path_to_dict<'py>(py: Python<'py>, path: &BomPath) -> PyResult<Bound<'py, PyDict>> {
    let item = PyDict::new(py);
    let path_type = path.path_type();
    let path_type_raw: u8 = path_type.into();
    let file_mode = path.file_mode();
    let symbolic_mode = symbolic_mode(path_type, file_mode);
    let symbolic_mode = std::str::from_utf8(&symbolic_mode)
        .map_err(|_| PyValueError::new_err("symbolic mode contained non-ASCII data"))?;
    let modified_time = path.modified_time();

    item.set_item(intern!(py, "path"), path.path())?;
    item.set_item(intern!(py, "path_type"), path_type_name(py, path_type))?;
    item.set_item(intern!(py, "path_type_raw"), path_type_raw)?;
    item.set_item(intern!(py, "file_mode"), file_mode)?;
    item.set_item(intern!(py, "symbolic_mode"), symbolic_mode)?;
    item.set_item(intern!(py, "user_id"), path.user_id())?;
    item.set_item(intern!(py, "group_id"), path.group_id())?;
    item.set_item(intern!(py, "mtime"), modified_time.timestamp())?;
    item.set_item(intern!(py, "mtime_iso8601"), modified_time.to_rfc3339())?;
    item.set_item(intern!(py, "size"), path.size())?;
    item.set_item(intern!(py, "crc32"), path.crc32())?;
    item.set_item(intern!(py, "link_name"), path.link_name())?;

    Ok(item)
}

fn path_record_fields<'py>(
    item: &Bound<'py, PyDict>,
    record: &apple_bom::format::BomBlockPathRecord<'_>,
) -> PyResult<()> {
    let path_type = BomPathType::from(record.path_type);

    item.set_item(
        intern!(item.py(), "path_type"),
        path_type_name(item.py(), path_type),
    )?;
    item.set_item(intern!(item.py(), "path_type_raw"), record.path_type)?;
    item.set_item(intern!(item.py(), "a"), record.a)?;
    item.set_item(intern!(item.py(), "architecture"), record.architecture)?;
    item.set_item(intern!(item.py(), "mode"), record.mode)?;
    item.set_item(intern!(item.py(), "user"), record.user)?;
    item.set_item(intern!(item.py(), "group"), record.group)?;
    item.set_item(intern!(item.py(), "mtime"), record.mtime)?;
    item.set_item(intern!(item.py(), "size"), record.size)?;
    item.set_item(intern!(item.py(), "b"), record.b)?;
    item.set_item(
        intern!(item.py(), "checksum_or_type"),
        record.checksum_or_type,
    )?;
    item.set_item(
        intern!(item.py(), "link_name_length"),
        record.link_name_length,
    )?;
    let link_name = record.link_name.as_ref().map(|name| name.to_string_lossy());
    item.set_item(intern!(item.py(), "link_name"), link_name)?;

    Ok(())
}

fn serialize_path_list<'py>(py: Python<'py>, paths: &[BomPath]) -> PyResult<Bound<'py, PyList>> {
    let items = paths
        .iter()
        .map(|path| path_to_dict(py, path))
        .collect::<PyResult<Vec<_>>>()?;
    PyList::new(py, items)
}

fn serialize_block_entry<'py>(
    py: Python<'py>,
    bom: &ParsedBom<'_>,
    index: usize,
    include_raw_block_bytes: bool,
) -> PyResult<Bound<'py, PyDict>> {
    let entry = bom.blocks.blocks.get(index).ok_or_else(|| {
        PyTypeError::new_err(format!(
            "block index {index} out of range while serializing"
        ))
    })?;

    let block_dict = PyDict::new(py);
    block_dict.set_item(intern!(py, "index"), index)?;
    block_dict.set_item(intern!(py, "file_offset"), entry.file_offset)?;
    block_dict.set_item(intern!(py, "length"), entry.length)?;

    let raw_data = bom.block_data(index).map_err(bom_error_to_py)?;

    if include_raw_block_bytes {
        block_dict.set_item(intern!(py, "raw_hex"), hex::encode(raw_data))?;
    }

    if raw_data.is_empty() {
        block_dict.set_item(intern!(py, "kind"), intern!(py, "Empty"))?;
        return Ok(block_dict);
    }

    // apple-bom's block type detector assumes at least 4 bytes for tree checks.
    if raw_data.len() < 4 {
        block_dict.set_item(intern!(py, "kind"), intern!(py, "Unknown"))?;
        block_dict.set_item(
            intern!(py, "parse_error"),
            "block too small for type detection",
        )?;
        return Ok(block_dict);
    }

    match catch_unwind(AssertUnwindSafe(|| {
        validation::parse_block_safely(bom, index)
    })) {
        Err(payload) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "Unknown"))?;
            block_dict.set_item(
                intern!(py, "parse_error"),
                format!(
                    "block parser panicked: {}",
                    panic_payload_to_string(payload)
                ),
            )?;
        }
        Ok(Err(err)) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "Unknown"))?;
            block_dict.set_item(intern!(py, "parse_error"), err.to_string())?;
        }
        Ok(Ok(BomBlock::Empty)) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "Empty"))?;
        }
        Ok(Ok(BomBlock::BomInfo(info))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "BomInfo"))?;
            block_dict.set_item(intern!(py, "version"), info.version)?;
            block_dict.set_item(intern!(py, "number_of_paths"), info.number_of_paths)?;
            block_dict.set_item(
                intern!(py, "number_of_info_entries"),
                info.number_of_info_entries,
            )?;

            let entry_items = info
                .entries
                .iter()
                .map(|info_entry| {
                    let item = PyDict::new(py);
                    item.set_item(intern!(py, "a"), info_entry.a)?;
                    item.set_item(intern!(py, "b"), info_entry.b)?;
                    item.set_item(intern!(py, "c"), info_entry.c)?;
                    item.set_item(intern!(py, "d"), info_entry.d)?;
                    Ok(item)
                })
                .collect::<PyResult<Vec<_>>>()?;
            let entries = PyList::new(py, entry_items)?;
            block_dict.set_item(intern!(py, "entries"), entries)?;
        }
        Ok(Ok(BomBlock::File(file))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "File"))?;
            block_dict.set_item(intern!(py, "parent_path_id"), file.parent_path_id)?;
            block_dict.set_item(intern!(py, "name"), file.name.to_string_lossy())?;
        }
        Ok(Ok(BomBlock::PathInfoIndex(path_info))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "PathInfoIndex"))?;
            block_dict.set_item(intern!(py, "path_id"), path_info.path_id)?;
            block_dict.set_item(
                intern!(py, "path_record_index"),
                path_info.path_record_index,
            )?;
        }
        Ok(Ok(BomBlock::PathRecord(record))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "PathRecord"))?;
            path_record_fields(&block_dict, &record)?;
        }
        Ok(Ok(BomBlock::PathRecordPointer(pointer))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "PathRecordPointer"))?;
            block_dict.set_item(
                intern!(py, "block_path_record_index"),
                pointer.block_path_record_index,
            )?;
        }
        Ok(Ok(BomBlock::Paths(paths))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "Paths"))?;
            block_dict.set_item(intern!(py, "is_path_info"), paths.is_path_info)?;
            block_dict.set_item(intern!(py, "count"), paths.count)?;
            block_dict.set_item(
                intern!(py, "next_paths_block_index"),
                paths.next_paths_block_index,
            )?;
            block_dict.set_item(
                intern!(py, "previous_paths_block_index"),
                paths.previous_paths_block_index,
            )?;

            let path_entry_items = paths
                .paths
                .iter()
                .map(|path| {
                    let item = PyDict::new(py);
                    item.set_item(intern!(py, "block_index"), path.block_index)?;
                    item.set_item(intern!(py, "file_index"), path.file_index)?;
                    Ok(item)
                })
                .collect::<PyResult<Vec<_>>>()?;
            let path_entries = PyList::new(py, path_entry_items)?;

            block_dict.set_item(intern!(py, "paths"), path_entries)?;
        }
        Ok(Ok(BomBlock::Tree(tree))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "Tree"))?;
            block_dict.set_item(intern!(py, "tree"), intern!(py, "tree"))?;
            block_dict.set_item(intern!(py, "version"), tree.version)?;
            block_dict.set_item(intern!(py, "block_paths_index"), tree.block_paths_index)?;
            block_dict.set_item(intern!(py, "block_size"), tree.block_size)?;
            block_dict.set_item(intern!(py, "path_count"), tree.path_count)?;
            block_dict.set_item(intern!(py, "a"), tree.a)?;
        }
        Ok(Ok(BomBlock::TreePointer(pointer))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "TreePointer"))?;
            block_dict.set_item(intern!(py, "block_tree_index"), pointer.block_tree_index)?;
        }
        Ok(Ok(BomBlock::VIndex(vindex))) => {
            block_dict.set_item(intern!(py, "kind"), intern!(py, "VIndex"))?;
            block_dict.set_item(intern!(py, "a"), vindex.a)?;
            block_dict.set_item(intern!(py, "tree_block_index"), vindex.tree_block_index)?;
            block_dict.set_item(intern!(py, "b"), vindex.b)?;
            block_dict.set_item(intern!(py, "c"), vindex.c)?;
        }
    }

    Ok(block_dict)
}

fn parse_optional_path_section<'py>(
    py: Python<'py>,
    doc: &Bound<'py, PyDict>,
    parse_errors: &Bound<'py, PyDict>,
    section: (&str, Result<Option<usize>, String>),
    bom: &ParsedBom<'_>,
    limits: ParseLimits,
    cache: &mut PathTreeCache,
) -> PyResult<()> {
    let (output_name, resolution) = section;
    let tree_index = match resolution {
        Ok(None) => {
            doc.set_item(output_name, py.None())?;
            return Ok(());
        }
        Err(err) => {
            doc.set_item(output_name, py.None())?;
            parse_errors.set_item(output_name, err)?;
            return Ok(());
        }
        Ok(Some(index)) => index,
    };

    let parsed = cache.parsed.entry(tree_index).or_insert_with(|| {
        match catch_unwind(AssertUnwindSafe(|| {
            validation::parse_path_tree(bom, tree_index, limits)
        })) {
            Ok(Ok(paths)) => ParsedPathTree::Value(paths),
            Ok(Err(err)) => ParsedPathTree::Error(err),
            Err(payload) => ParsedPathTree::Error(format!(
                "apple-bom parser panicked: {}",
                panic_payload_to_string(payload)
            )),
        }
    });

    match parsed {
        ParsedPathTree::Value(paths) => {
            doc.set_item(output_name, serialize_path_list(py, paths)?)?;
        }
        ParsedPathTree::Error(err) => {
            doc.set_item(output_name, py.None())?;
            parse_errors.set_item(output_name, err.as_str())?;
        }
    }

    let remove = if let Some(remaining) = cache.remaining_uses.get_mut(&tree_index) {
        *remaining = remaining.saturating_sub(1);
        *remaining == 0
    } else {
        false
    };
    if remove {
        cache.remaining_uses.remove(&tree_index);
        cache.parsed.remove(&tree_index);
    }

    Ok(())
}

fn parse_bom_document<'py>(
    py: Python<'py>,
    data: &[u8],
    source_path: Option<&str>,
    include_blocks: bool,
    include_raw_block_bytes: bool,
    limits: ParseLimits,
) -> PyResult<Bound<'py, PyDict>> {
    validation::validate_container(data, limits, include_blocks).map_err(BomParseError::new_err)?;

    let bom = match catch_unwind(AssertUnwindSafe(|| ParsedBom::parse(data))) {
        Ok(Ok(bom)) => bom,
        Ok(Err(err)) => return Err(bom_error_to_py(err)),
        Err(payload) => {
            return Err(BomParseError::new_err(format!(
                "apple-bom parser panicked: {}",
                panic_payload_to_string(payload)
            )))
        }
    };
    let doc = PyDict::new(py);
    let parse_errors = PyDict::new(py);

    doc.set_item(intern!(py, "format"), intern!(py, "apple-bom"))?;
    doc.set_item(intern!(py, "byte_length"), data.len())?;

    if let Some(path) = source_path {
        doc.set_item(intern!(py, "source_path"), path)?;
    }

    let header = PyDict::new(py);
    header.set_item(
        intern!(py, "magic"),
        String::from_utf8_lossy(&bom.header.magic),
    )?;
    header.set_item(intern!(py, "version"), bom.header.version)?;
    header.set_item(intern!(py, "number_of_blocks"), bom.header.number_of_blocks)?;
    header.set_item(
        intern!(py, "blocks_index_offset"),
        bom.header.blocks_index_offset,
    )?;
    header.set_item(
        intern!(py, "blocks_index_length"),
        bom.header.blocks_index_length,
    )?;
    header.set_item(
        intern!(py, "vars_index_offset"),
        bom.header.vars_index_offset,
    )?;
    header.set_item(
        intern!(py, "vars_index_length"),
        bom.header.vars_index_length,
    )?;
    doc.set_item(intern!(py, "header"), header)?;

    let blocks_index = PyDict::new(py);
    blocks_index.set_item(intern!(py, "count"), bom.blocks.count)?;
    let block_entry_items = bom
        .blocks
        .blocks
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let item = PyDict::new(py);
            item.set_item(intern!(py, "index"), index)?;
            item.set_item(intern!(py, "file_offset"), entry.file_offset)?;
            item.set_item(intern!(py, "length"), entry.length)?;
            Ok(item)
        })
        .collect::<PyResult<Vec<_>>>()?;
    let block_entries = PyList::new(py, block_entry_items)?;
    blocks_index.set_item(intern!(py, "entries"), block_entries)?;
    doc.set_item(intern!(py, "blocks_index"), blocks_index)?;

    let variable_items = bom
        .vars
        .vars
        .iter()
        .map(|var| {
            let item = PyDict::new(py);
            item.set_item(intern!(py, "name"), &var.name)?;
            item.set_item(intern!(py, "name_length"), var.name_length)?;
            item.set_item(intern!(py, "block_index"), var.block_index)?;
            Ok(item)
        })
        .collect::<PyResult<Vec<_>>>()?;
    let variables = PyList::new(py, variable_items)?;
    doc.set_item(intern!(py, "variables"), variables)?;

    match validation::validate_bom_info(&bom) {
        Ok(false) => {
            doc.set_item(intern!(py, "bom_info"), py.None())?;
        }
        Err(err) => {
            doc.set_item(intern!(py, "bom_info"), py.None())?;
            parse_errors.set_item(intern!(py, "bom_info"), err)?;
        }
        Ok(true) => match safe_bom_call(|| bom.bom_info()) {
            SafeBomCall::Value(info) => {
                let info_dict = PyDict::new(py);
                info_dict.set_item(intern!(py, "version"), info.version)?;
                info_dict.set_item(intern!(py, "number_of_paths"), info.number_of_paths)?;
                info_dict.set_item(
                    intern!(py, "number_of_info_entries"),
                    info.number_of_info_entries,
                )?;

                let entry_items = info
                    .entries
                    .iter()
                    .map(|info_entry| {
                        let item = PyDict::new(py);
                        item.set_item(intern!(py, "a"), info_entry.a)?;
                        item.set_item(intern!(py, "b"), info_entry.b)?;
                        item.set_item(intern!(py, "c"), info_entry.c)?;
                        item.set_item(intern!(py, "d"), info_entry.d)?;
                        Ok(item)
                    })
                    .collect::<PyResult<Vec<_>>>()?;
                let entries = PyList::new(py, entry_items)?;
                info_dict.set_item(intern!(py, "entries"), entries)?;

                doc.set_item(intern!(py, "bom_info"), info_dict)?;
            }
            SafeBomCall::MissingVariable => {
                doc.set_item(intern!(py, "bom_info"), py.None())?;
            }
            SafeBomCall::Error(err) => {
                doc.set_item(intern!(py, "bom_info"), py.None())?;
                parse_errors.set_item(intern!(py, "bom_info"), err)?;
            }
        },
    }

    let path_sections = [
        ("paths", validation::path_section_tree_index(&bom, "Paths")),
        (
            "hl_index",
            validation::path_section_tree_index(&bom, "HLIndex"),
        ),
        (
            "size64",
            validation::path_section_tree_index(&bom, "Size64"),
        ),
        (
            "vindex",
            validation::path_section_tree_index(&bom, "VIndex"),
        ),
    ];
    let mut remaining_uses = HashMap::with_capacity(4);
    for (_, resolution) in &path_sections {
        if let Ok(Some(tree_index)) = resolution {
            *remaining_uses.entry(*tree_index).or_insert(0) += 1;
        }
    }
    let mut path_cache = PathTreeCache {
        parsed: HashMap::with_capacity(remaining_uses.len()),
        remaining_uses,
    };
    for section in path_sections {
        parse_optional_path_section(
            py,
            &doc,
            &parse_errors,
            section,
            &bom,
            limits,
            &mut path_cache,
        )?;
    }

    if include_blocks {
        let block_items = (0..bom.blocks.blocks.len())
            .map(|index| serialize_block_entry(py, &bom, index, include_raw_block_bytes))
            .collect::<PyResult<Vec<_>>>()?;
        let blocks = PyList::new(py, block_items)?;
        doc.set_item(intern!(py, "blocks"), blocks)?;
    } else {
        doc.set_item(intern!(py, "blocks"), py.None())?;
    }

    if parse_errors.len() == 0 {
        doc.set_item(intern!(py, "parse_errors"), py.None())?;
    } else {
        doc.set_item(intern!(py, "parse_errors"), parse_errors)?;
    }

    Ok(doc)
}

fn parse_limits(max_input_bytes: usize, max_paths: usize) -> PyResult<ParseLimits> {
    if max_input_bytes == 0 {
        return Err(PyValueError::new_err(
            "max_input_bytes must be greater than zero",
        ));
    }
    if max_paths == 0 {
        return Err(PyValueError::new_err("max_paths must be greater than zero"));
    }

    Ok(ParseLimits {
        max_input_bytes,
        max_paths,
    })
}

fn read_file_limited(path: &str, max_input_bytes: usize) -> PyResult<Vec<u8>> {
    let file = File::open(path)
        .map_err(|err| PyOSError::new_err(format!("failed opening {path}: {err}")))?;
    let metadata = file
        .metadata()
        .map_err(|err| PyOSError::new_err(format!("failed reading metadata for {path}: {err}")))?;
    let max_input_u64 = u64::try_from(max_input_bytes).unwrap_or(u64::MAX);

    if metadata.len() > max_input_u64 {
        return Err(BomParseError::new_err(format!(
            "BOM input is {} bytes, exceeding max_input_bytes={max_input_bytes}",
            metadata.len()
        )));
    }

    let capacity = usize::try_from(metadata.len()).unwrap_or(max_input_bytes);
    let mut data = Vec::with_capacity(capacity);
    file.take(max_input_u64.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|err| PyOSError::new_err(format!("failed reading {path}: {err}")))?;

    if data.len() > max_input_bytes {
        return Err(BomParseError::new_err(format!(
            "BOM input exceeded max_input_bytes={max_input_bytes} while reading"
        )));
    }

    Ok(data)
}

#[pyfunction(signature = (data, *, include_blocks = true, include_raw_block_bytes = false, max_input_bytes = 134217728, max_paths = 250000))]
fn parse_bom_bytes<'py>(
    py: Python<'py>,
    data: &[u8],
    include_blocks: bool,
    include_raw_block_bytes: bool,
    max_input_bytes: usize,
    max_paths: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let limits = parse_limits(max_input_bytes, max_paths)?;
    parse_bom_document(
        py,
        data,
        None,
        include_blocks,
        include_raw_block_bytes,
        limits,
    )
}

#[pyfunction(signature = (path, *, include_blocks = true, include_raw_block_bytes = false, max_input_bytes = 134217728, max_paths = 250000))]
fn parse_bom_file<'py>(
    py: Python<'py>,
    path: &str,
    include_blocks: bool,
    include_raw_block_bytes: bool,
    max_input_bytes: usize,
    max_paths: usize,
) -> PyResult<Bound<'py, PyDict>> {
    let limits = parse_limits(max_input_bytes, max_paths)?;
    let data = read_file_limited(path, max_input_bytes)?;

    parse_bom_document(
        py,
        &data,
        Some(path),
        include_blocks,
        include_raw_block_bytes,
        limits,
    )
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    m.add("DEFAULT_MAX_INPUT_BYTES", DEFAULT_MAX_INPUT_BYTES)?;
    m.add("DEFAULT_MAX_PATHS", DEFAULT_MAX_PATHS)?;
    m.add("BomParseError", m.py().get_type::<BomParseError>())?;
    m.add_function(wrap_pyfunction!(parse_bom_bytes, m)?)?;
    m.add_function(wrap_pyfunction!(parse_bom_file, m)?)?;

    Ok(())
}
