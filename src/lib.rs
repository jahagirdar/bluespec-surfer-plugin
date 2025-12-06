use extism_pdk::{plugin_fn, FnResult, Json};
use serde::Deserialize;
use std::collections::HashMap;
pub use surfer_translation_types::plugin_types::TranslateParams;
use surfer_translation_types::{
    translator::{TrueName, VariableNameInfo},
    SubFieldTranslationResult, TranslationPreference, TranslationResult, ValueKind, VariableInfo,
    VariableMeta, VariableValue,
};

use extism_pdk::host_fn;
#[host_fn]
extern "ExtismHost" {
    pub fn read_file(filename: String) -> Vec<u8>;
    pub fn file_exists(filename: String) -> bool;
}
//static STATE: Mutex<bool> = Mutex::new(false);


// --- Output/Custom Structs ---

// --- JSON Deserialization Structs ---

#[derive(Deserialize, Debug)]
struct RawSegment {
    #[serde(rename = "var")]
    var_name: String,
    #[serde(rename = "type")] // <<< NEW FIELD
    type_name: String,        // <<< NEW FIELD
    min: isize,
    max: isize,
    width: usize,
}

// --- Output/Custom Structs ---

#[derive(Debug, Clone)]
pub struct TypeSegment {
    pub name: Option<String>,
    pub msb: usize,
    pub lsb: usize,
    pub type_name: String, // <<< NEW FIELD
}

// TypeStructure remains the same:
#[derive(Debug, Clone)]
pub struct TypeStructure {
    pub total_width: usize,
    pub segments: Vec<TypeSegment>,
}

// --- JSON Deserialization Structs ---


// Used within blocks
#[derive(Deserialize, Debug)]
struct RawBlockPort {
    #[serde(rename = "var")]
    name: String,
    #[serde(rename = "type")]
    type_name: String,
}

// Used within blocks
#[derive(Deserialize, Debug)]
struct RawBlock {
    #[serde(rename = "var")]
    module_name: String,
    ports: Vec<RawBlockPort>,
}

// **FileContent: Unifying typedefs and blocks**
#[derive(Deserialize, Debug)]
struct FileContent {
    typedefs: HashMap<String, Vec<RawSegment>>,
    blocks: Vec<RawBlock>,
}

use once_cell::sync::Lazy;
use std::sync::Mutex;

// Assuming TypeStructure and TypeBlock (HashMap<String, String>) are defined/aliased

// Static variable to hold the processed Type Definitions
static BSV_TYPES: Lazy<Mutex<HashMap<String, TypeStructure>>> = Lazy::new(|| {
    // Initialize with an empty HashMap inside a Mutex
    Mutex::new(HashMap::new())
});

// Static variable to hold the processed Block-to-Type map
static BSV_BLOCKS: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| {
    // Initialize with an empty HashMap inside a Mutex
    Mutex::new(HashMap::new())
});

pub fn process_typedefs(
    raw_typedefs: HashMap<String, Vec<RawSegment>>
) -> Result<HashMap<String, TypeStructure>, Box<dyn std::error::Error>> {

    let mut bsv_types: HashMap<String, TypeStructure> = HashMap::new();
    let ignored_types = ["Bits", "bool"];

    for (type_name, raw_segments) in raw_typedefs {
        if ignored_types.iter().any(|&s| type_name.contains(s)) {
            continue;
        }

        let mut total_width = 0;
        let mut segments: Vec<TypeSegment> = Vec::new();

        for segment in raw_segments {

            // Sum the 'width' field of each segment to get the total width
            total_width += segment.width;

            let segment_name = if segment.var_name.is_empty() {
                None
            } else {
                Some(segment.var_name)
            };

            // Create the new TypeSegment, including the type_name
            segments.push(TypeSegment {
                name: segment_name,
                msb: segment.max.abs() as usize,
                lsb: segment.min.abs() as usize,
                type_name: segment.type_name, // <<< MAPPING THE NEW FIELD
            });
        }

        bsv_types.insert(
            type_name,
            TypeStructure {
                total_width,
                segments,
            },
        );
    }
    Ok(bsv_types)
}
const IGNORED_PORTS: [&str; 3] = ["CLK", "RST", "EN"];
const PREFERRED_PORTS: [&str; 3] = ["Q_OUT", "Probe", "D_IN"];

// Renamed the function and changed its return type to handle error propagation
// and update the global statics.
pub fn process_blocks(
    raw_blocks: Vec<RawBlock>
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {

    let mut block_types_map: HashMap<String, String> = HashMap::new();

    for block in raw_blocks {
        let module_name = block.module_name;

        let mut selected_type: Option<String> = None;
        let mut best_port_rank: isize = -1;

        for port in block.ports {
            let port_name = port.name;

            if IGNORED_PORTS.contains(&port_name.as_str()) {
                continue;
            }

            let current_rank: Option<usize> = PREFERRED_PORTS.iter().position(|&p| p == port_name.as_str());

            if let Some(rank) = current_rank {
                let new_rank = -(rank as isize);

                if new_rank > best_port_rank {
                    selected_type = Some(port.type_name);
                    best_port_rank = new_rank;
                }

                if best_port_rank == 0 { // Optimization: Q_OUT is found
                    break;
                }
            }
        }

        if let Some(type_name) = selected_type {
            block_types_map.insert(module_name, type_name);
        }
    }

    Ok(block_types_map)
}
// Renamed the function and changed its return type to handle error propagation
// and update the global statics.
pub fn process_bluespec_file() -> Result<(), Box<dyn std::error::Error>> {
    let filename = "bluespec.json".to_string();

    // 1. File Read & Deserialization
    let json_bytes: Vec<u8> = unsafe {
        // Assuming read_file returns Vec<u8> based on your declaration
        read_file(filename)?
    };

    if json_bytes.is_empty() {
        return Err(Box::<dyn std::error::Error>::from("Failed to read bluespec.json or file is empty."));
    }

    let file_content: FileContent = serde_json::from_slice(&json_bytes)?;

    // 2. Process Data
    let bsv_types = process_typedefs(file_content.typedefs)?;
    let module_types = process_blocks(file_content.blocks)?;
    extism_pdk::warn!("bsv_types = {:?}",bsv_types);
    extism_pdk::warn!("module_types = {:?}",module_types);

    // 3. Update Static Variables (Requires locking the Mutex)

    // Acquire the lock for TypeDefs and replace the contents
    let mut types_guard = BSV_TYPES.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_TYPES: {}", e))?; // Handle Mutex poisoning error
    *types_guard = bsv_types;

    // Acquire the lock for Blocks and replace the contents
    let mut blocks_guard = BSV_BLOCKS.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_BLOCKS: {}", e))?; // Handle Mutex poisoning error
    *blocks_guard = module_types;

    Ok(())
}
#[plugin_fn]
    pub fn new() -> FnResult<()> {
        // 1. Call the initialization function
        match process_bluespec_file() {
            Ok(_) => {
                // Initialization successful
                Ok(())
            },
            Err(e) => {
                // Initialization failed. Log the error and return an error result.
                // In an Extism PDK environment, you'd use the `extism_pdk::log` function.
                // For a simple standard error, you can use `extism_pdk::extism::error`.
                // The simplest way to return an error is using the FnResult return type.
                eprintln!("Error initializing static data: {}", e);

                // Return an error from the plugin function
               // Err(extism_pdk::Error::msg(format!("Initialization failed: {}", e)))
                Err(extism_pdk::Error::msg(format!("Initialization failed: {}", e)).into())

            }
        }
    }

#[plugin_fn]
pub fn name() -> FnResult<String> {
    Ok("Bluespec Translator".to_string())
}

#[plugin_fn]
pub fn translates(_variable: VariableMeta<(), ()>) -> FnResult<TranslationPreference> {
    extism_pdk::warn!("translates variable {:?} ",_variable);
    if(_variable.var.name == "current_format"){
    Ok(TranslationPreference::Prefer)
    } else {
    Ok(TranslationPreference::No)
    }
}
#[plugin_fn]
pub fn translate_old(
    TranslateParams { variable, value }: TranslateParams,
) -> FnResult<TranslationResult> {
    extism_pdk::warn!("translate variable {:?} ",variable);
    let binary_digits = match value {
        VariableValue::BigUint(big_uint) => {
            let raw = format!("{big_uint:b}");
            let padding = (0..((variable.num_bits.unwrap_or_default() as usize)
                .saturating_sub(raw.len())))
                .map(|_| "0")
                .collect::<Vec<_>>()
                .join("");

            format!("{padding}{raw}")
        }
        VariableValue::String(v) => v.clone(),
    };

    let digits = binary_digits.chars().collect::<Vec<_>>();

    Ok(TranslationResult {
        val: surfer_translation_types::ValueRepr::Tuple,
        subfields: {
            digits
                .chunks(4)
                .enumerate()
                .map(|(i, chunk)| SubFieldTranslationResult {
                    name: format!("[{i}]"),
                    result: TranslationResult {
                        val: surfer_translation_types::ValueRepr::Bits(4, chunk.iter().collect()),
                        subfields: vec![],
                        kind: ValueKind::Normal,
                    },
                })
                .collect()
        },
        kind: ValueKind::Normal,
    })
}
// Assuming necessary use statements are present:
// use extism_pdk::prelude::*;
// use once_cell::sync::Lazy;
// use std::sync::Mutex;
// use std::collections::HashMap;
// use crate::bsv_parser::{TypeStructure, TypeSegment}; // Or wherever your structs are defined
// use crate::surfer_translation_types::{TranslationResult, SubFieldTranslationResult, ValueRepr, ValueKind, TranslateParams, VariableValue}; // And the types defined by the host

// --- Static Data Definitions (Assumed to be defined elsewhere in your crate) ---
// extern crate once_cell;
// use once_cell::sync::Lazy;
// use std::sync::Mutex;
// use std::collections::HashMap;
//
// // BSV_BLOCKS: maps variable name (e.g., "xspi_inst_mkFifo_i") -> BSV Type Name (e.g., "xSPITypes::Format_st")
// pub static BSV_BLOCKS: Lazy<Mutex<HashMap<String, String>>> = Lazy::new(|| Mutex::new(HashMap::new()));
//
// // BSV_TYPES: maps BSV Type Name (e.g., "xSPITypes::Format_st") -> TypeStructure
// pub static BSV_TYPES: Lazy<Mutex<HashMap<String, TypeStructure>>> = Lazy::new(|| Mutex::new(HashMap::new()));
// -------------------------------------------------------------------------------

fn strip_port_suffix(name: &str) -> String {
    // Try to locate the last '_' or '$'
    if let Some(pos) = name.rfind(|c| c == '_' || c == '$') {
        let suffix = &name[pos + 1 ..];

        // Rule 1: direct match handles "Probe" exactly
        let direct_match = PREFERRED_PORTS.contains(&suffix);

        // Rule 2: uppercase match handles Q_OUT, D_IN, etc.
        let uppercase_match =
            suffix.chars().all(|c| c.is_ascii_uppercase()) &&
            PREFERRED_PORTS.iter().any(|p| p.eq_ignore_ascii_case(suffix));

        if direct_match || uppercase_match {
            return name[..pos].to_string();
        }
    }

    name.to_string()
}


#[plugin_fn]
pub fn translate(
    TranslateParams { variable, value }: TranslateParams,
) -> FnResult<TranslationResult> {

    // Log variable info for debugging
    extism_pdk::warn!("translate variable {:?} ", variable);

    // Match on the value to determine the translation strategy
    match value {

        // --- Compound Type Handling (BigUint) ---
        VariableValue::BigUint(big_uint) => {

            // 1. Prepare for structure lookup
            let raw_block_key = variable.var.name.as_str();
            let block_key = strip_port_suffix(raw_block_key);

            // let block_key = variable.var.name.as_str();

            // Acquire locks for reading the static maps
            let bsv_blocks_map = BSV_BLOCKS.lock().unwrap();
            let bsv_types_map = BSV_TYPES.lock().unwrap();

            let bsv_type_name_option = bsv_blocks_map.get(&block_key);

            // Check if we found a corresponding structure type
            if let Some(bsv_type_name) = bsv_type_name_option {

                if let Some(type_structure) = bsv_types_map.get(bsv_type_name) {

                    // 2. Prepare the full binary string, padded to the structure's total width
                    let raw = format!("{big_uint:b}");
                    let total_bits = type_structure.total_width;
                    let padding_len = total_bits.saturating_sub(raw.len());
                    let padding = (0..padding_len).map(|_| '0').collect::<String>();
                    let padded_binary = format!("{padding}{raw}");
                    let digits_vec: Vec<char> = padded_binary.chars().collect();

                    let mut subfields: Vec<SubFieldTranslationResult> = Vec::new();

                    // 3. Segment the binary value based on the TypeStructure
                    for segment in &type_structure.segments {

                        // Calculate start (MSB) and end (LSB) indices in the `digits_vec`.
                        // Indices are 0-based from the MSB (left).
                        // Example: total_bits=16. Segment MSB=15, LSB=8.
                        // MSB index: 16 - 1 - 15 = 0
                        // LSB index: 16 - 1 - 8 = 7
                        // Slice: digits_vec[0..8]

                        let msb_index = total_bits.saturating_sub(1).saturating_sub(segment.msb);
                        let lsb_index = total_bits.saturating_sub(1).saturating_sub(segment.lsb);

                        let start_idx = msb_index;
                        let end_idx = lsb_index.saturating_add(1);
                        let segment_width = segment.msb.saturating_sub(segment.lsb).saturating_add(1);

                        if start_idx < digits_vec.len() && end_idx <= digits_vec.len() && start_idx <= end_idx {
                            let chunk: String = digits_vec[start_idx..end_idx].iter().collect();

                            // Use the segment's name (e.g., "cmd")
                            let name = segment.name.clone().unwrap_or_else(||
                                format!("{}_Bits_{}-{}", bsv_type_name, segment.msb, segment.lsb)
                            );

                            subfields.push(SubFieldTranslationResult {
                                name,
                                result: TranslationResult {
                                    // The inner value is simply the extracted bits
                                    val: surfer_translation_types::ValueRepr::Bits(segment_width as u64, chunk),
                                    subfields: vec![],
                                    kind: ValueKind::Normal,
                                },
                            });
                        } else {
                             extism_pdk::warn!("Segment index calculation failed for '{}' ({}-{}) in total width {}",
                                segment.name.as_deref().unwrap_or("unnamed"),
                                segment.msb, segment.lsb, total_bits
                            );
                        }
                    }

                    // Return the structured (Tuple) result
                    return Ok(TranslationResult {
                        val: surfer_translation_types::ValueRepr::Tuple,
                        subfields,
                        kind: ValueKind::Normal,
                    });

                } else {
                    // Type structure found in BSV_BLOCKS but not defined in BSV_TYPES
                    extism_pdk::warn!("Type structure '{}' found in BSV_BLOCKS but missing from BSV_TYPES. Falling back to simple Bits view.", bsv_type_name);
                }
            } else {
                // No mapping found in BSV_BLOCKS for this variable.
                extism_pdk::warn!("Variable '{}' not found in BSV_BLOCKS. Falling back to simple Bits view.", block_key);
            }

            // Fallback: If no structure or an error occurred in lookup, return the simple Bits representation (original logic)
            let raw = format!("{big_uint:b}");
            let width = variable.num_bits.unwrap_or_default() as usize;
            let padding_len = width.saturating_sub(raw.len());
            let padding = (0..padding_len).map(|_| '0').collect::<String>();
            let padded_binary = format!("{padding}{raw}");

            Ok(TranslationResult {
                val: surfer_translation_types::ValueRepr::Bits(width as u64, padded_binary),
                subfields: vec![],
                kind: ValueKind::Normal,
            })
        },

        // --- String Type Handling ---
        VariableValue::String(v) => {
            Ok(TranslationResult {
                val: surfer_translation_types::ValueRepr::String(v.clone()),
                subfields: vec![],
                kind: ValueKind::Normal,
            })
        },

        // --- Default/Unsupported Types ---
        _ => Err(extism_pdk::Error::msg("Unsupported VariableValue type for translation.").into()),
    }
}

// #[plugin_fn]
// pub fn variable_info(variable: VariableMeta<(), ()>) -> FnResult<VariableInfo> {
//     extism_pdk::warn!("variable_info variable {:?} ",variable);
//     Ok(VariableInfo::Compound {
//         subfields: (0..(variable.num_bits.unwrap_or_default() / 4 + 1))
//             .map(|i| (format!("[{i}]"), VariableInfo::Bits))
//             .collect(),
//     })
// }
#[plugin_fn]
pub fn variable_info(variable: VariableMeta<(), ()>) -> FnResult<VariableInfo> {
    extism_pdk::warn!("variable_info variable {:?} ", variable);

    // Determine the lookup key for this variable
    let block_key = variable.var.name.as_str();

    // Acquire the same maps used by translate()
    let bsv_blocks_map = BSV_BLOCKS.lock().unwrap();
    let bsv_types_map = BSV_TYPES.lock().unwrap();

    // If this variable corresponds to a structured BSV block…
    if let Some(bsv_type_name) = bsv_blocks_map.get(block_key) {
        if let Some(type_structure) = bsv_types_map.get(bsv_type_name) {

            // ---- Structured case: output a tuple of named bitfields ----
            let mut subfields = Vec::new();

            for segment in &type_structure.segments {
                // Name: segment.name OR auto-generated fallback
                let name = segment.name.clone().unwrap_or_else(|| {
                    format!("{}_Bits_{}-{}", bsv_type_name, segment.msb, segment.lsb)
                });

                let width = segment
                    .msb
                    .saturating_sub(segment.lsb)
                    .saturating_add(1) as u64;

                subfields.push((name, VariableInfo::Bits {  }));
            }

            return Ok(VariableInfo::Compound { subfields });
        }
    }

    // ---- Fallback 1: No BSV structure → simple Bits(width) ----
    if let Some(width) = variable.num_bits {
        return Ok(VariableInfo::Bits { });
    }

    // ---- Fallback 2: Unknown → treat as opaque/bits ----
    Ok(VariableInfo::Bits {  })
}



#[plugin_fn]
pub fn variable_name_info(
    Json(variable): Json<VariableMeta<(), ()>>,
) -> FnResult<Option<VariableNameInfo>> {
    extism_pdk::warn!("variable_name_info variable {:?} ",variable);
    let result = match variable.var.name.as_str() {
        "trace_data" => Some(VariableNameInfo {
            true_name: Some(TrueName::SourceCode {
                line_number: 1,
                before: "ab".to_string(),
                this: "cde".to_string(),
                after: "ef".to_string(),
            }),
            priority: Some(2),
        }),
        "trace_file" => Some(VariableNameInfo {
            true_name: Some(TrueName::SourceCode {
                line_number: 2,
                before: "this is a very long start of line".to_string(),
                this: "short".to_string(),
                after: "a".to_string(),
            }),
            priority: Some(0),
        }),
        "trace_valid" => Some(VariableNameInfo {
            true_name: Some(TrueName::SourceCode {
                line_number: 3,
                before: "a".to_string(),
                this: "trace_valid".to_string(),
                after: "this is a very long end of line".to_string(),
            }),
            priority: Some(0),
        }),
        "resetn" => Some(VariableNameInfo {
            true_name: Some(TrueName::SourceCode {
                line_number: 4,
                before: "this is a very long start of line".to_string(),
                this: "resetn".to_string(),
                after: "this is a very long end of line".to_string(),
            }),
            priority: Some(-1),
        }),
        "clk" => Some(VariableNameInfo {
            true_name: Some(TrueName::SourceCode {
                line_number: 555,
                before: "this is a very long start of line".to_string(),
                this: "clk is a very long signal name that stretches".to_string(),
                after: "this is a very long end of line".to_string(),
            }),
            priority: Some(0),
        }),
        _ => None,
    };
    Ok(result)
}
