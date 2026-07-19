use apple_bom::{
    format::{BomBlock, ParsedBom},
    BomPathType,
};
use std::{
    collections::{HashMap, HashSet},
    ffi::CStr,
    ops::Range,
};

pub(crate) const DEFAULT_MAX_INPUT_BYTES: usize = 128 * 1024 * 1024;
pub(crate) const DEFAULT_MAX_PATHS: usize = 250_000;

const BOM_HEADER_LEN: usize = 32;
const BLOCKS_INDEX_HEADER_LEN: usize = 4;
const BLOCK_INDEX_ENTRY_LEN: usize = 8;
const VARS_INDEX_HEADER_LEN: usize = 4;
const VAR_INDEX_ENTRY_HEADER_LEN: usize = 5;
const BOM_INFO_HEADER_LEN: usize = 12;
const BOM_INFO_ENTRY_LEN: usize = 16;
const PATHS_HEADER_LEN: usize = 12;
const PATHS_ENTRY_LEN: usize = 8;
const PATH_RECORD_HEADER_LEN: usize = 31;
const TREE_LEN: usize = 21;
const VINDEX_LEN: usize = 13;
const MAX_VARIABLES: usize = 65_536;
const MAX_PATH_DEPTH: usize = 4_096;
const MIN_MAX_BLOCKS: usize = DEFAULT_MAX_PATHS * BLOCKS_PER_PATH_LIMIT;
const BLOCKS_PER_PATH_LIMIT: usize = 4;
const MIN_TOTAL_PATH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) struct ParseLimits {
    pub(crate) max_input_bytes: usize,
    pub(crate) max_paths: usize,
}

impl ParseLimits {
    fn max_blocks(self) -> usize {
        self.max_paths
            .saturating_mul(BLOCKS_PER_PATH_LIMIT)
            .max(MIN_MAX_BLOCKS)
    }

    fn max_total_path_bytes(self) -> usize {
        self.max_input_bytes
            .saturating_mul(4)
            .max(MIN_TOTAL_PATH_BYTES)
    }
}

#[derive(Clone, Copy)]
struct PathsHeader {
    is_path_info: u16,
    count: usize,
    next_index: usize,
}

#[derive(Clone, Copy)]
struct PathMetric {
    full_len: usize,
    depth: usize,
    serialized_len: usize,
    resolution_work: usize,
}

pub(crate) fn validate_container(
    data: &[u8],
    limits: ParseLimits,
    include_block_output: bool,
) -> Result<(), String> {
    if data.len() > limits.max_input_bytes {
        return Err(format!(
            "BOM input is {} bytes, exceeding max_input_bytes={}",
            data.len(),
            limits.max_input_bytes
        ));
    }

    if data.len() < BOM_HEADER_LEN {
        return Err(format!(
            "BOM header is truncated: expected at least {BOM_HEADER_LEN} bytes, got {}",
            data.len()
        ));
    }

    if data.get(..8) != Some(b"BOMStore".as_slice()) {
        return Err("invalid BOM magic; expected BOMStore".to_string());
    }

    let blocks_range = index_range(data, 16, 20, "blocks index")?;
    let vars_range = index_range(data, 24, 28, "variables index")?;

    let block_count = validate_blocks_index(
        data,
        blocks_range,
        limits.max_blocks(),
        include_block_output.then_some(limits.max_input_bytes),
    )?;
    validate_vars_index(data, vars_range, block_count)?;

    Ok(())
}

pub(crate) fn validate_bom_info(bom: &ParsedBom<'_>) -> Result<bool, String> {
    let Some(index) = variable_block_index(bom, "BomInfo") else {
        return Ok(false);
    };

    let data = block_data(bom, index)?;
    if !bom_info_layout_is_safe(data) {
        return Err("BomInfo block has an invalid entry count or is truncated".to_string());
    }

    Ok(true)
}

pub(crate) fn validate_path_section(
    bom: &ParsedBom<'_>,
    name: &str,
    limits: ParseLimits,
) -> Result<bool, String> {
    let Some(variable_index) = variable_block_index(bom, name) else {
        return Ok(false);
    };

    let tree_index = if name == "VIndex" {
        let data = block_data(bom, variable_index)?;
        if data.len() < VINDEX_LEN {
            return Err("VIndex block is truncated".to_string());
        }
        read_u32(data, 4, "VIndex tree block index")? as usize
    } else {
        variable_index
    };

    validate_path_tree(bom, tree_index, limits)?;
    Ok(true)
}

pub(crate) fn parse_block_safely<'a>(
    bom: &'a ParsedBom<'a>,
    index: usize,
) -> Result<BomBlock<'a>, String> {
    let data = block_data(bom, index)?;

    if index == 1 && bom_info_layout_is_safe(data) {
        if let Ok(info) = bom.block_as_bom_info(index) {
            return Ok(BomBlock::BomInfo(info));
        }
    }

    if tree_layout_is_safe(data) {
        if let Ok(tree) = bom.block_as_tree(index) {
            if paths_block_at_is_safe(bom, tree.block_paths_index as usize)
                && tree.paths(bom).is_ok()
            {
                return Ok(BomBlock::Tree(tree));
            }
        }
    }

    if paths_layout_is_safe(data) {
        if let Ok(paths) = bom.block_as_paths(index) {
            if paths
                .paths
                .iter()
                .all(|entry| path_entry_layout_is_safe(bom, entry.block_index, entry.file_index))
            {
                return Ok(BomBlock::Paths(paths));
            }
        }
    }

    if data.len() >= VINDEX_LEN {
        if let Ok(vindex) = bom.block_as_vindex(index) {
            if tree_block_at_is_safe(bom, vindex.tree_block_index as usize)
                && vindex.tree(bom).is_ok()
            {
                return Ok(BomBlock::VIndex(vindex));
            }
        }
    }

    if data.len() >= 8 {
        if let Ok(path_info) = bom.block_as_path_info_index(index) {
            if path_record_block_at_is_safe(bom, path_info.path_record_index as usize)
                && path_info.path_record(bom).is_ok()
            {
                return Ok(BomBlock::PathInfoIndex(path_info));
            }
        }
    }

    if path_record_layout_is_safe(data) {
        if let Ok(record) = bom.block_as_path_record(index) {
            return Ok(BomBlock::PathRecord(record));
        }
    }

    if file_layout_is_safe(data) {
        if let Ok(file) = bom.block_as_file(index) {
            return Ok(BomBlock::File(file));
        }
    }

    if data.len() >= 4 {
        if let Ok(pointer) = bom.block_as_path_record_pointer(index) {
            if path_record_block_at_is_safe(bom, pointer.block_path_record_index as usize)
                && pointer.path_record(bom).is_ok()
            {
                return Ok(BomBlock::PathRecordPointer(pointer));
            }
        }

        if let Ok(pointer) = bom.block_as_tree_pointer(index) {
            if tree_block_at_is_safe(bom, pointer.block_tree_index as usize)
                && pointer.tree(bom).is_ok()
            {
                return Ok(BomBlock::TreePointer(pointer));
            }
        }
    }

    if bom_info_layout_is_safe(data) {
        if let Ok(info) = bom.block_as_bom_info(index) {
            return Ok(BomBlock::BomInfo(info));
        }
    }

    Err("unknown or malformed block type".to_string())
}

fn validate_blocks_index(
    data: &[u8],
    range: Range<usize>,
    max_blocks: usize,
    max_total_raw_bytes: Option<usize>,
) -> Result<usize, String> {
    let index = data
        .get(range.clone())
        .ok_or_else(|| "blocks index is outside the input".to_string())?;
    if index.len() < BLOCKS_INDEX_HEADER_LEN {
        return Err("blocks index is truncated".to_string());
    }

    let count = read_u32(index, 0, "blocks count")? as usize;
    if count > max_blocks {
        return Err(format!(
            "blocks index contains {count} entries, exceeding the limit of {max_blocks}"
        ));
    }

    let entries_len = count
        .checked_mul(BLOCK_INDEX_ENTRY_LEN)
        .and_then(|value| value.checked_add(BLOCKS_INDEX_HEADER_LEN))
        .ok_or_else(|| "blocks index size overflows the platform size".to_string())?;
    if entries_len > index.len() {
        return Err(format!(
            "blocks index declares {count} entries but only {} bytes are available",
            index.len()
        ));
    }

    let mut total_raw_bytes = 0usize;
    for entry_index in 0..count {
        let offset = BLOCKS_INDEX_HEADER_LEN + entry_index * BLOCK_INDEX_ENTRY_LEN;
        let file_offset_raw = read_u32(index, offset, "block file offset")?;
        let length_raw = read_u32(index, offset + 4, "block length")?;
        file_offset_raw
            .checked_add(length_raw)
            .ok_or_else(|| "block data range overflows the BOM offset width".to_string())?;
        let file_offset = file_offset_raw as usize;
        let length = length_raw as usize;
        checked_range(data.len(), file_offset, length, "block data")?;

        if let Some(max_total) = max_total_raw_bytes {
            total_raw_bytes = total_raw_bytes
                .checked_add(length)
                .ok_or_else(|| "total raw block size overflows the platform size".to_string())?;
            if total_raw_bytes > max_total {
                return Err(format!(
                    "block output would process {total_raw_bytes} bytes, exceeding the limit of {max_total}"
                ));
            }
        }
    }

    Ok(count)
}

fn validate_vars_index(data: &[u8], range: Range<usize>, block_count: usize) -> Result<(), String> {
    let index = data
        .get(range)
        .ok_or_else(|| "variables index is outside the input".to_string())?;
    if index.len() < VARS_INDEX_HEADER_LEN {
        return Err("variables index is truncated".to_string());
    }

    let count = read_u32(index, 0, "variables count")? as usize;
    if count > MAX_VARIABLES {
        return Err(format!(
            "variables index contains {count} entries, exceeding the limit of {MAX_VARIABLES}"
        ));
    }
    let minimum_len = count
        .checked_mul(VAR_INDEX_ENTRY_HEADER_LEN)
        .and_then(|value| value.checked_add(VARS_INDEX_HEADER_LEN))
        .ok_or_else(|| "variables index size overflows the platform size".to_string())?;
    if minimum_len > index.len() {
        return Err(format!(
            "variables index declares {count} entries but only {} bytes are available",
            index.len()
        ));
    }

    let mut offset = VARS_INDEX_HEADER_LEN;
    for _ in 0..count {
        let header_end = offset
            .checked_add(VAR_INDEX_ENTRY_HEADER_LEN)
            .ok_or_else(|| "variable entry offset overflows the platform size".to_string())?;
        if header_end > index.len() {
            return Err("variable entry header is truncated".to_string());
        }

        let block_index = read_u32(index, offset, "variable block index")? as usize;
        if block_index >= block_count {
            return Err(format!(
                "variable references block {block_index}, but the blocks index has {block_count} entries"
            ));
        }

        let name_len = usize::from(index[offset + 4]);
        let name_start = header_end;
        let name_end = name_start
            .checked_add(name_len)
            .ok_or_else(|| "variable name length overflows the platform size".to_string())?;
        let name = index
            .get(name_start..name_end)
            .ok_or_else(|| "variable name is truncated".to_string())?;
        std::str::from_utf8(name).map_err(|_| "variable name is not valid UTF-8".to_string())?;
        offset = name_end;
    }

    Ok(())
}

fn validate_path_tree(
    bom: &ParsedBom<'_>,
    tree_index: usize,
    limits: ParseLimits,
) -> Result<(), String> {
    let tree = block_data(bom, tree_index)?;
    if !tree_layout_is_safe(tree) {
        return Err(format!("tree block {tree_index} is malformed or truncated"));
    }

    let path_count = read_u32(tree, 16, "tree path count")? as usize;
    if path_count > limits.max_paths {
        return Err(format!(
            "tree declares {path_count} paths, exceeding max_paths={}",
            limits.max_paths
        ));
    }

    let mut paths_index = read_u32(tree, 8, "tree paths block index")? as usize;
    let mut root_seen = HashSet::new();
    loop {
        if !root_seen.insert(paths_index) {
            return Err(format!(
                "cycle detected while resolving root paths block {paths_index}"
            ));
        }
        if root_seen.len() > bom.blocks.blocks.len() {
            return Err("root paths traversal exceeds the block count".to_string());
        }

        let data = block_data(bom, paths_index)?;
        let header = parse_paths_header(data)?;
        if header.is_path_info != 0 {
            break;
        }
        if header.count == 0 {
            return Err(format!(
                "root paths block {paths_index} has no child path blocks"
            ));
        }
        paths_index = read_u32(data, PATHS_HEADER_LEN, "child paths block index")? as usize;
    }

    let mut linked_seen = HashSet::new();
    let mut files_by_id = HashMap::new();
    let mut total_paths = 0usize;
    let mut total_path_bytes = 0usize;
    let mut total_resolution_work = 0usize;

    loop {
        if !linked_seen.insert(paths_index) {
            return Err(format!(
                "cycle detected in linked paths blocks at block {paths_index}"
            ));
        }
        if linked_seen.len() > bom.blocks.blocks.len() {
            return Err("linked paths traversal exceeds the block count".to_string());
        }

        let data = block_data(bom, paths_index)?;
        let header = parse_paths_header(data)?;
        if header.is_path_info == 0 {
            return Err(format!(
                "linked paths block {paths_index} does not contain path records"
            ));
        }

        total_paths = total_paths
            .checked_add(header.count)
            .ok_or_else(|| "path count overflows the platform size".to_string())?;
        if total_paths > limits.max_paths {
            return Err(format!(
                "tree contains more than max_paths={} path records",
                limits.max_paths
            ));
        }

        for entry_index in 0..header.count {
            let offset = PATHS_HEADER_LEN + entry_index * PATHS_ENTRY_LEN;
            let path_info_index = read_u32(data, offset, "path info block index")? as usize;
            let file_index = read_u32(data, offset + 4, "file block index")? as usize;
            let (path_id, metric) =
                validate_path_entry(bom, path_info_index, file_index, &files_by_id)?;

            if files_by_id.insert(path_id, metric).is_some() {
                return Err(format!("duplicate path identifier {path_id}"));
            }
            total_path_bytes = total_path_bytes
                .checked_add(metric.serialized_len)
                .ok_or_else(|| "total path size overflows the platform size".to_string())?;
            if total_path_bytes > limits.max_total_path_bytes() {
                return Err(format!(
                    "expanded paths exceed the {} byte resource limit",
                    limits.max_total_path_bytes()
                ));
            }
            total_resolution_work = total_resolution_work
                .checked_add(metric.resolution_work)
                .ok_or_else(|| "path resolution work overflows the platform size".to_string())?;
            if total_resolution_work > limits.max_total_path_bytes() {
                return Err(format!(
                    "path resolution work exceeds the {} byte resource limit",
                    limits.max_total_path_bytes()
                ));
            }
        }

        if header.next_index == 0 {
            break;
        }
        paths_index = header.next_index;
    }

    Ok(())
}

fn validate_path_entry(
    bom: &ParsedBom<'_>,
    path_info_index: usize,
    file_index: usize,
    files_by_id: &HashMap<u32, PathMetric>,
) -> Result<(u32, PathMetric), String> {
    let path_info = block_data(bom, path_info_index)?;
    if path_info.len() < 8 {
        return Err(format!("path info block {path_info_index} is truncated"));
    }
    let path_id = read_u32(path_info, 0, "path identifier")?;
    let path_record_index = read_u32(path_info, 4, "path record block index")? as usize;
    let path_record = block_data(bom, path_record_index)?;
    if !path_record_layout_is_safe(path_record) {
        return Err(format!(
            "path record block {path_record_index} is malformed or truncated"
        ));
    }

    let file = block_data(bom, file_index)?;
    if !file_layout_is_safe(file) {
        return Err(format!("file block {file_index} is malformed or truncated"));
    }
    let parent_id = read_u32(file, 0, "parent path identifier")?;
    let name_bytes = file
        .get(4..file.len().saturating_sub(1))
        .ok_or_else(|| format!("file block {file_index} has an invalid name"))?;
    let name_len = name_bytes
        .len()
        .checked_mul(3)
        .ok_or_else(|| "path name length overflows the platform size".to_string())?;

    let (full_len, depth, resolution_work) = if parent_id == 0 {
        (name_len, 1, name_len)
    } else {
        let parent = files_by_id
            .get(&parent_id)
            .ok_or_else(|| format!("path references unknown parent identifier {parent_id}"))?;
        let depth = parent
            .depth
            .checked_add(1)
            .ok_or_else(|| "path depth overflows the platform size".to_string())?;
        if depth > MAX_PATH_DEPTH {
            return Err(format!(
                "path hierarchy exceeds the maximum depth of {MAX_PATH_DEPTH}"
            ));
        }
        let full_len = parent
            .full_len
            .checked_add(1)
            .and_then(|value| value.checked_add(name_len))
            .ok_or_else(|| "expanded path length overflows the platform size".to_string())?;
        // The upstream parser rebuilds the path once for every ancestor. Account
        // for all those intermediate strings so a deep hierarchy cannot turn a
        // small final output into quadratic work per path.
        let repeated_leaf_work = name_len
            .checked_add(1)
            .and_then(|value| value.checked_mul(parent.depth))
            .ok_or_else(|| "path resolution work overflows the platform size".to_string())?;
        let resolution_work = name_len
            .checked_add(repeated_leaf_work)
            .and_then(|value| value.checked_add(parent.resolution_work))
            .ok_or_else(|| "path resolution work overflows the platform size".to_string())?;
        (full_len, depth, resolution_work)
    };

    let link_len = if path_record[0] == u8::from(BomPathType::Link) {
        (read_u32(path_record, 27, "link name length")? as usize)
            .checked_mul(3)
            .ok_or_else(|| "expanded link name length overflows the platform size".to_string())?
    } else {
        0
    };
    let serialized_len = full_len
        .checked_add(link_len)
        .ok_or_else(|| "serialized path length overflows the platform size".to_string())?;

    let metric = PathMetric {
        full_len,
        depth,
        serialized_len,
        resolution_work,
    };

    Ok((path_id, metric))
}

fn path_entry_layout_is_safe(bom: &ParsedBom<'_>, path_info: u32, file: u32) -> bool {
    let Ok(path_info_data) = block_data(bom, path_info as usize) else {
        return false;
    };
    if path_info_data.len() < 8 {
        return false;
    }
    let Ok(record_index) = read_u32(path_info_data, 4, "path record block index") else {
        return false;
    };

    path_record_block_at_is_safe(bom, record_index as usize)
        && block_data(bom, file as usize).is_ok_and(file_layout_is_safe)
}

fn parse_paths_header(data: &[u8]) -> Result<PathsHeader, String> {
    if !paths_layout_is_safe(data) {
        return Err("paths block has an invalid entry count or is truncated".to_string());
    }

    Ok(PathsHeader {
        is_path_info: read_u16(data, 0, "paths block type")?,
        count: read_u16(data, 2, "paths count")? as usize,
        next_index: read_u32(data, 4, "next paths block index")? as usize,
    })
}

fn bom_info_layout_is_safe(data: &[u8]) -> bool {
    let Ok(count) = read_u32(data, 8, "BomInfo entry count") else {
        return false;
    };
    let Some(required) = (count as usize)
        .checked_mul(BOM_INFO_ENTRY_LEN)
        .and_then(|value| value.checked_add(BOM_INFO_HEADER_LEN))
    else {
        return false;
    };
    required <= data.len()
}

fn paths_layout_is_safe(data: &[u8]) -> bool {
    let Ok(count) = read_u16(data, 2, "paths count") else {
        return false;
    };
    let Some(required) = (count as usize)
        .checked_mul(PATHS_ENTRY_LEN)
        .and_then(|value| value.checked_add(PATHS_HEADER_LEN))
    else {
        return false;
    };
    required <= data.len()
}

fn path_record_layout_is_safe(data: &[u8]) -> bool {
    if data.len() < PATH_RECORD_HEADER_LEN {
        return false;
    }
    if data[0] != u8::from(BomPathType::Link) {
        return true;
    }

    let Ok(link_len) = read_u32(data, 27, "link name length") else {
        return false;
    };
    if link_len == 0 {
        return true;
    }
    let Some(end) = PATH_RECORD_HEADER_LEN.checked_add(link_len as usize) else {
        return false;
    };
    data.get(PATH_RECORD_HEADER_LEN..end)
        .is_some_and(|value| CStr::from_bytes_with_nul(value).is_ok())
}

fn file_layout_is_safe(data: &[u8]) -> bool {
    data.get(4..)
        .is_some_and(|value| CStr::from_bytes_with_nul(value).is_ok())
}

fn tree_layout_is_safe(data: &[u8]) -> bool {
    data.len() >= TREE_LEN && data.get(..4) == Some(b"tree".as_slice())
}

fn paths_block_at_is_safe(bom: &ParsedBom<'_>, index: usize) -> bool {
    block_data(bom, index).is_ok_and(paths_layout_is_safe)
}

fn path_record_block_at_is_safe(bom: &ParsedBom<'_>, index: usize) -> bool {
    block_data(bom, index).is_ok_and(path_record_layout_is_safe)
}

fn tree_block_at_is_safe(bom: &ParsedBom<'_>, index: usize) -> bool {
    block_data(bom, index).is_ok_and(tree_layout_is_safe)
}

fn variable_block_index(bom: &ParsedBom<'_>, name: &str) -> Option<usize> {
    bom.vars
        .vars
        .iter()
        .find(|variable| variable.name == name)
        .map(|variable| variable.block_index as usize)
}

fn block_data<'a>(bom: &'a ParsedBom<'_>, index: usize) -> Result<&'a [u8], String> {
    let entry = bom
        .blocks
        .blocks
        .get(index)
        .ok_or_else(|| format!("block index {index} is out of range"))?;
    entry
        .file_offset
        .checked_add(entry.length)
        .ok_or_else(|| "block data range overflows the BOM offset width".to_string())?;
    let range = checked_range(
        bom.data.len(),
        entry.file_offset as usize,
        entry.length as usize,
        "block data",
    )?;
    bom.data
        .get(range)
        .ok_or_else(|| format!("block index {index} is outside the input"))
}

fn index_range(
    data: &[u8],
    offset_field: usize,
    length_field: usize,
    label: &str,
) -> Result<Range<usize>, String> {
    let offset_raw = read_u32(data, offset_field, label)?;
    let length_raw = read_u32(data, length_field, label)?;
    offset_raw
        .checked_add(length_raw)
        .ok_or_else(|| format!("{label} range overflows the BOM offset width"))?;
    checked_range(data.len(), offset_raw as usize, length_raw as usize, label)
}

fn checked_range(
    data_len: usize,
    offset: usize,
    length: usize,
    label: &str,
) -> Result<Range<usize>, String> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| format!("{label} range overflows the platform size"))?;
    if end > data_len {
        return Err(format!(
            "{label} range {offset}..{end} exceeds input length {data_len}"
        ));
    }
    Ok(offset..end)
}

fn read_u16(data: &[u8], offset: usize, label: &str) -> Result<u16, String> {
    let end = offset
        .checked_add(2)
        .ok_or_else(|| format!("{label} offset overflows the platform size"))?;
    let bytes: [u8; 2] = data
        .get(offset..end)
        .ok_or_else(|| format!("{label} is truncated"))?
        .try_into()
        .map_err(|_| format!("{label} has an invalid width"))?;
    Ok(u16::from_be_bytes(bytes))
}

fn read_u32(data: &[u8], offset: usize, label: &str) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| format!("{label} offset overflows the platform size"))?;
    let bytes: [u8; 4] = data
        .get(offset..end)
        .ok_or_else(|| format!("{label} is truncated"))?
        .try_into()
        .map_err(|_| format!("{label} has an invalid width"))?;
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &[u8] = include_bytes!("../tests/fixtures/python-applications.bom");

    fn default_limits() -> ParseLimits {
        ParseLimits {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            max_paths: DEFAULT_MAX_PATHS,
        }
    }

    fn deep_path_bom(path_count: u16) -> Result<Vec<u8>, String> {
        const TREE_INDEX: u32 = 0;
        const PATHS_INDEX: u32 = 1;
        const RECORD_INDEX: u32 = 2;

        let mut tree = Vec::with_capacity(TREE_LEN);
        tree.extend_from_slice(b"tree");
        tree.extend_from_slice(&1_u32.to_be_bytes());
        tree.extend_from_slice(&PATHS_INDEX.to_be_bytes());
        tree.extend_from_slice(&4_096_u32.to_be_bytes());
        tree.extend_from_slice(&u32::from(path_count).to_be_bytes());
        tree.push(0);

        let mut paths =
            Vec::with_capacity(PATHS_HEADER_LEN + usize::from(path_count) * PATHS_ENTRY_LEN);
        paths.extend_from_slice(&1_u16.to_be_bytes());
        paths.extend_from_slice(&path_count.to_be_bytes());
        paths.extend_from_slice(&0_u32.to_be_bytes());
        paths.extend_from_slice(&0_u32.to_be_bytes());

        let mut record = vec![0; PATH_RECORD_HEADER_LEN];
        record[0] = u8::from(BomPathType::File);
        let mut blocks = vec![tree, paths, record];

        for index in 0..path_count {
            let path_id = u32::from(index) + 1;
            let path_info_index = u32::try_from(blocks.len())
                .map_err(|_| "path info block index does not fit in u32".to_string())?;
            let file_index = path_info_index + 1;
            blocks[PATHS_INDEX as usize].extend_from_slice(&path_info_index.to_be_bytes());
            blocks[PATHS_INDEX as usize].extend_from_slice(&file_index.to_be_bytes());

            let mut path_info = Vec::with_capacity(8);
            path_info.extend_from_slice(&path_id.to_be_bytes());
            path_info.extend_from_slice(&RECORD_INDEX.to_be_bytes());
            blocks.push(path_info);

            let mut file = Vec::with_capacity(6);
            file.extend_from_slice(&u32::from(index).to_be_bytes());
            file.extend_from_slice(b"a\0");
            blocks.push(file);
        }

        let block_count = u32::try_from(blocks.len())
            .map_err(|_| "block count does not fit in u32".to_string())?;
        let blocks_index_len = BLOCKS_INDEX_HEADER_LEN
            .checked_add(blocks.len() * BLOCK_INDEX_ENTRY_LEN)
            .ok_or_else(|| "blocks index length overflow".to_string())?;
        let vars_index_len = VARS_INDEX_HEADER_LEN + VAR_INDEX_ENTRY_HEADER_LEN + "Paths".len();
        let blocks_index_offset = BOM_HEADER_LEN;
        let vars_index_offset = blocks_index_offset + blocks_index_len;
        let payload_offset = vars_index_offset + vars_index_len;

        let mut data = Vec::new();
        data.extend_from_slice(b"BOMStore");
        data.extend_from_slice(&1_u32.to_be_bytes());
        data.extend_from_slice(&block_count.to_be_bytes());
        for value in [
            blocks_index_offset,
            blocks_index_len,
            vars_index_offset,
            vars_index_len,
        ] {
            let value = u32::try_from(value)
                .map_err(|_| "container offset does not fit in u32".to_string())?;
            data.extend_from_slice(&value.to_be_bytes());
        }

        data.extend_from_slice(&block_count.to_be_bytes());
        let mut block_offset = payload_offset;
        for block in &blocks {
            let offset = u32::try_from(block_offset)
                .map_err(|_| "block offset does not fit in u32".to_string())?;
            let length = u32::try_from(block.len())
                .map_err(|_| "block length does not fit in u32".to_string())?;
            data.extend_from_slice(&offset.to_be_bytes());
            data.extend_from_slice(&length.to_be_bytes());
            block_offset = block_offset
                .checked_add(block.len())
                .ok_or_else(|| "block offset overflow".to_string())?;
        }

        data.extend_from_slice(&1_u32.to_be_bytes());
        data.extend_from_slice(&TREE_INDEX.to_be_bytes());
        data.push(5);
        data.extend_from_slice(b"Paths");
        for block in blocks {
            data.extend_from_slice(&block);
        }

        Ok(data)
    }

    #[test]
    fn fixture_passes_container_and_section_validation() -> Result<(), String> {
        validate_container(FIXTURE, default_limits(), true)?;
        let bom = ParsedBom::parse(FIXTURE).map_err(|error| error.to_string())?;

        assert!(validate_bom_info(&bom)?);
        assert!(validate_path_section(&bom, "Paths", default_limits())?);
        assert!(validate_path_section(&bom, "HLIndex", default_limits())?);
        assert!(validate_path_section(&bom, "Size64", default_limits())?);
        assert!(validate_path_section(&bom, "VIndex", default_limits())?);

        for index in 0..bom.blocks.blocks.len() {
            let _ = parse_block_safely(&bom, index);
        }
        Ok(())
    }

    #[test]
    fn rejects_truncated_and_bad_magic_inputs() {
        let truncated = validate_container(b"BOMStore", default_limits(), false);
        assert!(truncated.is_err());

        let mut bad_magic = FIXTURE.to_vec();
        bad_magic[0] = b'X';
        let invalid = validate_container(&bad_magic, default_limits(), false);
        assert!(invalid.is_err());
    }

    #[test]
    fn rejects_oversized_block_count_before_allocation() {
        let mut data = FIXTURE.to_vec();
        let blocks_offset = read_u32(&data, 16, "blocks offset").map_or(0, |value| value as usize);
        if let Some(count) = data.get_mut(blocks_offset..blocks_offset.saturating_add(4)) {
            count.copy_from_slice(&u32::MAX.to_be_bytes());
        }

        let result = validate_container(&data, default_limits(), false);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_path_block_cycle() -> Result<(), String> {
        validate_container(FIXTURE, default_limits(), false)?;
        let original = ParsedBom::parse(FIXTURE).map_err(|error| error.to_string())?;
        let variable_index = variable_block_index(&original, "Paths")
            .ok_or_else(|| "fixture has no Paths variable".to_string())?;
        let tree = block_data(&original, variable_index)?;
        let mut paths_index = read_u32(tree, 8, "tree paths block index")? as usize;

        loop {
            let data = block_data(&original, paths_index)?;
            let header = parse_paths_header(data)?;
            if header.is_path_info != 0 {
                break;
            }
            paths_index = read_u32(data, PATHS_HEADER_LEN, "child paths block index")? as usize;
        }

        let entry = original
            .blocks
            .blocks
            .get(paths_index)
            .ok_or_else(|| "paths block index is out of range".to_string())?;
        let next_offset = (entry.file_offset as usize)
            .checked_add(4)
            .ok_or_else(|| "next block offset overflow".to_string())?;
        let mut data = FIXTURE.to_vec();
        let next = data
            .get_mut(next_offset..next_offset + 4)
            .ok_or_else(|| "next block field is truncated".to_string())?;
        next.copy_from_slice(&(paths_index as u32).to_be_bytes());

        validate_container(&data, default_limits(), false)?;
        let bom = ParsedBom::parse(&data).map_err(|error| error.to_string())?;
        let result = validate_path_section(&bom, "Paths", default_limits());
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    fn rejects_quadratic_path_resolution_work() -> Result<(), String> {
        let data = deep_path_bom(500)?;
        let limits = ParseLimits {
            max_input_bytes: data.len(),
            max_paths: DEFAULT_MAX_PATHS,
        };
        validate_container(&data, limits, false)?;
        let bom = ParsedBom::parse(&data).map_err(|error| error.to_string())?;

        let result = validate_path_section(&bom, "Paths", limits);
        let Err(error) = result else {
            return Err("deep path hierarchy unexpectedly passed validation".to_string());
        };
        assert!(error.contains("path resolution work"));
        Ok(())
    }
}
