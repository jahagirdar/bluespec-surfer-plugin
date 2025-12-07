use extism_pdk::{plugin_fn, FnResult, Json, Error};
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


// --- JSON Deserialization Structs for maps.json ---

// Inner struct for the value: {mkTop:[main, top]}
// Maps a module name (e.g., "mkTop") to a list of original names (e.g., ["main", "top"])
#[derive(Deserialize, Debug)]
struct ModuleMapping(HashMap<String, Vec<String>>);

// **ModuleMapContent: maps variable type/instance name to a ModuleMapping**
// e.g., "top": {mkTop:[main, top]}
#[derive(Deserialize, Debug)]
struct ModuleMapContent {
    // Top-level key is the module instance name (e.g., "top")
    // Value is the ModuleMapping struct defined above
    #[serde(flatten)]
    maps: HashMap<String, ModuleMapping>,
}

// Static variable to hold the determined current module name (String) or "undef" if not found
static CURRENT_MODULE: Lazy<Mutex<Option<String>>> = Lazy::new(|| { Mutex::new(None) });

// Static variable to hold the processed Module Name Maps
// Maps Variable Instance Name (String) to Module:OriginalName Mapping (HashMap<String, Vec<String>>)
static BSV_MAPS: Lazy<Mutex<HashMap<String, HashMap<String, Vec<String>>>>> = Lazy::new(|| {
    Mutex::new(HashMap::new())
});

// --- JSON Deserialization Structs (Primitives) ---
#[derive(Deserialize, Debug)]
struct RawSegment {
    #[serde(rename = "var")]
    var_name: String,
    #[serde(rename = "type")]
    type_name: String,
    min: isize,
    max: isize,
    width: usize,
}

// Used within blocks
#[derive(Deserialize, Debug)]
struct RawBlockPort {
    #[serde(rename = "var")]
    name: String,
    #[serde(rename = "type")]
    type_name: String,
}

// *** NEW: Module Block Definition (Value in the "blocks" HashMap) ***
#[derive(Deserialize, Debug)]
struct ModuleBlock {
    #[serde(rename = "type")]
    type_name: String,
    ports: Vec<RawBlockPort>,
}

// *** NEW: Content within a single module (e.g., "mkTop") ***
#[derive(Deserialize, Debug)]
struct ModuleContent {
    typedefs: HashMap<String, Vec<RawSegment>>,
    // Blocks is now a HashMap keyed by instance name
    blocks: HashMap<String, ModuleBlock>, 
}

// *** NEW: Root structure for the entire file ***
#[derive(Deserialize, Debug)]
struct DesignFile {
    top: String, // The name of the top module (e.g., "mkTop")
    // Use serde(flatten) to catch all other dynamic keys (the module names)
    #[serde(flatten)]
    modules: HashMap<String, ModuleContent>, 
}


// --- Output/Custom Structs (Processed) ---

#[derive(Debug, Clone)]
pub struct TypeSegment {
    pub name: Option<String>,
    pub msb: usize,
    pub lsb: usize,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct TypeStructure {
    pub total_width: usize,
    pub segments: Vec<TypeSegment>,
}

// *** NEW: The single structure holding all processed data for a module ***
#[derive(Debug, Clone)]
pub struct ModuleData {
    // Maps Type Name -> Structure
    pub typedefs: HashMap<String, TypeStructure>, 
    // Maps Block Instance Name -> BSV Type Name
    pub blocks: HashMap<String, String>,         
}


use once_cell::sync::Lazy;
use std::sync::Mutex;

// *** NEW: Single static map for all modules ***
// Static variable to hold the processed Type Definitions and Blocks, scoped by module
static BSV_MODULES: Lazy<Mutex<HashMap<String, ModuleData>>> = Lazy::new(|| {
    Mutex::new(HashMap::new())
});

// In lib.rs, near your processing functions
use extism_pdk::{warn }; // Ensure 'Error' is imported

/// Checks if a file exists, reads it if it does, and returns the content or an empty vector on failure.
/// All existence and read errors are logged via warn!
pub fn read_bsv_file(filename: &str) -> Vec<u8> {

    // 1. Check if file exists.
    // We use a match statement to handle the actual Result<bool, Error> returned by the host.
    let exists = match unsafe { file_exists(filename.to_string()) } {
        Ok(b) => b,
        Err(e) => {
            // Log the error and treat existence check failure as 'not found'.
            warn!("Error checking existence of '{}': {}. Assuming file does not exist.", filename, e);
            return Vec::new(); // Return empty Vec to signal failure
        }
    };

    if !exists {
        warn!("File '{}' not found (file_exists returned false). Skipping read.", filename);
        return Vec::new();
    }

    // 2. Read the file.
    // We use a match statement to handle the actual Result<Vec<u8>, Error> returned by the host.
    let json_bytes: Vec<u8> = match unsafe { read_file(filename.to_string()) } {
        Ok(bytes) => bytes,
        Err(e) => {
            // Log the error and return an empty vector on read failure.
            warn!("Error reading file '{}': {}. Returning empty content.", filename, e);
            return Vec::new();
        }
    };

    // 3. Manual check for empty content (empty content could still be a soft error).
    if json_bytes.is_empty() {
        warn!("File '{}' was read successfully but is empty.", filename);
    }

    // Returns the bytes (content or empty vector).
    json_bytes
}

pub fn process_module_maps() -> Result<(), Box<dyn std::error::Error>> {
    let filename = "bluespec_map.json".to_string();

    let json_bytes=read_bsv_file(&filename);
    // Explicitly handle deserialization failure
    let file_content: ModuleMapContent = match serde_json::from_slice(&json_bytes) {
        Ok(content) => content,
        Err(e) => {
            // This log will print if the JSON is malformed
            extism_pdk::warn!("bluespec_map.json deserialization failed: {}. Skipping module map loading.", e);
            return Ok(());
        }
    };
    
    // 2. Process Data: Flatten the ModuleMapping struct into the target HashMap
    let mut module_maps: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    for (instance_name, ModuleMapping(mapping)) in file_content.maps {
        module_maps.insert(instance_name, mapping);
    }

    // This log should now print if deserialization was successful.
    extism_pdk::warn!("module_maps loaded with {} entries.", module_maps.len()); 

    // 3. Update Static Variable
    let mut maps_guard = BSV_MAPS.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_MAPS: {}", e))?; 
    *maps_guard = module_maps;

    Ok(())
}

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
                type_name: segment.type_name,
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

// *** MODIFIED: Function signature now accepts HashMap<String, ModuleBlock> ***
pub fn process_blocks(
    raw_blocks: HashMap<String, ModuleBlock> 
) -> Result<HashMap<String, String>, Box<dyn std::error::Error>> {

    let mut block_types_map: HashMap<String, String> = HashMap::new();

    // Iterate over (instance_name, block_content) pairs
    for (instance_name, block) in raw_blocks {
        let module_instance_name = instance_name; // The instance name is now the key

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
            // The key is the instance name
            block_types_map.insert(module_instance_name, type_name); 
        }
    }

    Ok(block_types_map)
}

pub fn process_bluespec_map() -> Result<(), Box<dyn std::error::Error>> {
    let filename = "bluespec_map.json".to_string();

    let json_bytes = read_bsv_file(&filename);

    let file_content: ModuleMapContent = serde_json::from_slice(&json_bytes)?;

    // 2. Process Data: Flatten the ModuleMapping struct into the target HashMap
    let mut module_maps: HashMap<String, HashMap<String, Vec<String>>> = HashMap::new();
    for (instance_name, ModuleMapping(mapping)) in file_content.maps {
        module_maps.insert(instance_name, mapping);
    }
    extism_pdk::warn!("module_maps = {:?}", module_maps);

    // 3. Update Static Variable (Requires locking the Mutex)

    // Acquire the lock for BSV_MAPS and replace the contents
    let mut maps_guard = BSV_MAPS.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_MAPS: {}", e))?;
    *maps_guard = module_maps;

    Ok(())
}
// *** MODIFIED: Logic to iterate over modules and populate BSV_MODULES ***
pub fn process_bluespec_file() -> Result<(), Box<dyn std::error::Error>> {
    let filename = "bluespec.json".to_string();

    // 1. File Read & Deserialization
    let json_bytes: Vec<u8> = unsafe {
        read_file(filename)?
    };

    if json_bytes.is_empty() {
        return Err(Box::<dyn std::error::Error>::from("Failed to read bluespec.json or file is empty."));
    }

    // Deserialize into the new root struct
    let file_content: DesignFile = serde_json::from_slice(&json_bytes)?; 

    // 2. Process Data - Collect data scoped by module
    let mut all_modules: HashMap<String, ModuleData> = HashMap::new();

    for (module_name, module_content) in file_content.modules {
        // Process typedefs
        let bsv_types = process_typedefs(module_content.typedefs)?;
        
        // Process blocks
        let module_blocks = process_blocks(module_content.blocks)?; 

        // Assemble the ModuleData struct
        let module_data = ModuleData {
            typedefs: bsv_types,
            blocks: module_blocks,
        };
        
        // Insert into the main map
        all_modules.insert(module_name, module_data);
    }
    
    extism_pdk::warn!("BSV_MODULES aggregated: {:?}", all_modules);

    // 3. Update Static Variables (Requires locking the Mutex)
    let mut modules_guard = BSV_MODULES.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_MODULES: {}", e))?;
    *modules_guard = all_modules; 

    Ok(())
}
// New top-level function to manage all initialization steps.
pub fn initialize_static_data() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Load Type Definitions and Blocks from bluespec.json
    process_bluespec_file()?;

    // 2. Load Module Maps from maps.json
    process_bluespec_map()?;

    Ok(())
}

#[plugin_fn]
    pub fn new() -> FnResult<()> {
        match initialize_static_data() {
            Ok(_) => Ok(()),
            Err(e) => {
                eprintln!("Error initializing static data: {}", e);
                Err(extism_pdk::Error::msg(format!("Initialization failed: {}", e)).into())
            }
        }
    }

#[plugin_fn]
pub fn name() -> FnResult<String> {
    Ok("Bluespec Translator".to_string())
}

// Helper function to resolve the VCD scope path to the final BSV module name.
pub fn get_current_module(
    scope_path: &[String]
) -> Result<String, Box<dyn std::error::Error>> {

    if scope_path.is_empty() {
        return Err(Box::<dyn std::error::Error>::from("Scope path is empty."));
    }

    let bsv_maps = BSV_MAPS.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_MAPS: {}", e))?;
    let bsv_modules = BSV_MODULES.lock()
        .map_err(|e| format!("Mutex poisoned for BSV_MODULES: {}", e))?;

    let mut current_module_name: Option<String> = None;
    let mut path_start_index: usize = 0;

    // --- Phase 1: Initial Scope Resolution (Using BSV_MAPS) ---
    // Find the BSV module name corresponding to the start of the VCD path.
    for (_bsv_instance_name, module_mapping) in bsv_maps.iter() {
        for (module_impl_name, original_names) in module_mapping.iter() {

            // Check if the original name list matches the start of the scope path
            if scope_path.len() >= original_names.len() && &scope_path[..original_names.len()] == original_names.as_slice() {
                // Match found! The base module name is the implementation name (e.g., "mkTop").
                current_module_name = Some(module_impl_name.clone());
                path_start_index = original_names.len();
                break;
            }
        }
        if current_module_name.is_some() {
            break;
        }
    }

    let mut module_name = match current_module_name {
        Some(name) => name,
        None => {
            return Err(Box::<dyn std::error::Error>::from(
                format!("Initial scope path {:?} not found in BSV_MAPS lookup.", scope_path)
            ));
        }
    };

    // --- Phase 2: Traverse Remaining Path (Using BSV_MODULES) ---
    // Example: Path elements remaining: ["ab_inst_a"]
    for next_instance in scope_path.iter().skip(path_start_index) {

        // 1. Find the ModuleData for the current module name
        let module_data = match bsv_modules.get(&module_name) {
            Some(data) => data,
            None => {
                return Err(Box::<dyn std::error::Error>::from(
                    format!("Module '{}' not found in BSV_MODULES.", module_name)
                ));
            }
        };

        // 2. Lookup the next instance in the current module's blocks to get its BSV type name
        let bsv_type_name = match module_data.blocks.get(next_instance) {
            Some(name) => name, // This is the type name of the sub-module instance
            None => {
                // If the next path element isn't an instance/block, it might be a segment of a type.
                // We stop here and use the last successfully resolved module name.
                // NOTE: Based on the request's original logic, we should probably stop and assume failure if we can't traverse.
                return Err(Box::<dyn std::error::Error>::from(
                    format!("Sub-module instance '{}' not found in blocks of module type '{}'.", next_instance, module_name)
                ));
            }
        };

        // The *type name* of the block (e.g., "xSPITypes::Format_st") becomes the *module name* for the next iteration.
        // This relies on the assumption that a module's Type Name is also the key for its ModuleContent in the JSON.
        module_name = bsv_type_name.clone();
    }

    // Step 3: Successfully resolved to the final module name
    Ok(module_name)
}
#[plugin_fn]
pub fn translates(_variable: VariableMeta<(), ()>) -> FnResult<TranslationPreference> {
    extism_pdk::warn!("translates variable {:?} ",_variable);

    let scope_path = _variable.var.path.strs.as_slice();
    let variable_name = _variable.var.name.as_str();

    // 1. Resolve Current Module
    let result = get_current_module(scope_path);

    // Acquire lock for CURRENT_MODULE
    let mut current_module_guard = CURRENT_MODULE.lock().unwrap();

    let preference = match result {
        Ok(final_module_name) => {
            // Resolution successful. Store the module name.
            extism_pdk::warn!("Resolved scope {:?} to module: {}", scope_path, final_module_name);
            *current_module_guard = Some(final_module_name.clone());

            // 2. Check for Variable Existence in Final Module Structure

            let bsv_modules = BSV_MODULES.lock().unwrap();

            // a. Get the ModuleData for the resolved module name
            if let Some(module_data) = bsv_modules.get(&final_module_name) {

                // b. The variable we are checking must be a segment of a typedef in this module.
                // It means the variable name must be the key of a type in the blocks map.

                // For a variable 'rb', its type is the key in the module's 'blocks' map.
                if let Some(bsv_type_name) = module_data.blocks.get(variable_name) {

                    // c. Check if the type name has a structure defined in this module's typedefs
                    if module_data.typedefs.contains_key(bsv_type_name) {
                        extism_pdk::warn!("Variable '{}' is a block/instance with known type '{}'. Preferring translation.", variable_name, bsv_type_name);
                        TranslationPreference::Prefer
                    } else {
                        // Type is known, but structure is not (e.g., it's "Bits" or not defined locally)
                        extism_pdk::warn!("Variable '{}' block type found, but structure is missing. No translation.", variable_name);
                        TranslationPreference::No
                    }
                } else {
                    // Variable name is not a block/instance name in this module.
                    extism_pdk::warn!("Variable '{}' not found as a block in module '{}'. No translation.", variable_name, final_module_name);
                    TranslationPreference::No
                }
            } else {
                // Resolved module name not found in BSV_MODULES (shouldn't happen if Phase 1/2 worked, but for safety)
                extism_pdk::warn!("Final module '{}' not found in BSV_MODULES. No translation.", final_module_name);
                TranslationPreference::No
            }
        },
        Err(e) => {
            // Resolution failed. Log error and set current module to None.
            extism_pdk::warn!("Scope resolution failed: {}. No translation.", e);
            *current_module_guard = None; // Set to None on error
            TranslationPreference::No
        }
    };

    Ok(preference)
}
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

            let mut bsv_type_name_option: Option<String> = None;
            let mut type_structure_option: Option<&TypeStructure> = None;

            // *** START MODIFIED LOOKUP ***

            // 2. Acquire locks for CURRENT_MODULE and BSV_MODULES
            let current_module_guard = CURRENT_MODULE.lock().unwrap();
            let bsv_modules_map = BSV_MODULES.lock().unwrap();
            
            // 3. Perform targeted lookup using CURRENT_MODULE
            if let Some(module_name) = current_module_guard.as_ref() {

                extism_pdk::warn!("Targeting current module: {}", module_name);
                
                // a. Get the specific ModuleData for the CURRENT_MODULE
                if let Some(module_data) = bsv_modules_map.get(module_name) {
                    
                    // b. Check if the block key (instance name) exists in this module's blocks
                    if let Some(bsv_type_name) = module_data.blocks.get(&block_key) {
                        extism_pdk::warn!("Found!!! {:?} {:?}", bsv_type_name, block_key);
                        
                        // c. If found, look up its type structure within this module's typedefs
                        if let Some(type_structure) = module_data.typedefs.get(bsv_type_name) {
                            extism_pdk::warn!("Found!!! {:?} ", type_structure);
                            
                            bsv_type_name_option = Some(bsv_type_name.clone());
                            type_structure_option = Some(type_structure);
                        }
                    }
                }
            } else {
                extism_pdk::warn!("CURRENT_MODULE is None. Skipping structured lookup.");
            }

            // *** END MODIFIED LOOKUP ***

            // Check if we found a corresponding structure type
            if let (Some(bsv_type_name), Some(type_structure)) = (bsv_type_name_option, type_structure_option) {

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
                // No mapping found in the CURRENT_MODULE for this variable.
                extism_pdk::warn!("Variable '{}' not found in CURRENT_MODULE structure. Falling back to simple Bits view.", block_key);
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


#[plugin_fn]
pub fn variable_info(variable: VariableMeta<(), ()>) -> FnResult<VariableInfo> {
    extism_pdk::warn!("variable_info variable {:?} ", variable);

    // Determine the lookup key for this variable
    let block_key = variable.var.name.as_str();

    // *** MODIFIED LOOKUP: Acquire lock on the single map ***
    let bsv_modules_map = BSV_MODULES.lock().unwrap();

    let mut bsv_type_name_option: Option<String> = None;
    let mut type_structure_option: Option<&TypeStructure> = None;

    // Iterate through all modules to find a match for the block_key
    for (_module_name, module_data) in bsv_modules_map.iter() {
        
        if let Some(bsv_type_name) = module_data.blocks.get(block_key) {
            
            if let Some(type_structure) = module_data.typedefs.get(bsv_type_name) {
                
                bsv_type_name_option = Some(bsv_type_name.clone());
                type_structure_option = Some(type_structure);
                
                // Found the match, exit the module loop
                break; 
            }
        }
    }
    // *** END MODIFIED LOOKUP ***

    // If this variable corresponds to a structured BSV block…
    if let (Some(bsv_type_name), Some(type_structure)) = (bsv_type_name_option, type_structure_option) {

        // ---- Structured case: output a tuple of named bitfields ----
        let mut subfields = Vec::new();

        for segment in &type_structure.segments {
            // Name: segment.name OR auto-generated fallback
            let name = segment.name.clone().unwrap_or_else(|| {
                format!("{}_Bits_{}-{}", bsv_type_name, segment.msb, segment.lsb)
            });

            let _width = segment
                .msb
                .saturating_sub(segment.lsb)
                .saturating_add(1) as u64;

            subfields.push((name, VariableInfo::Bits {  }));
        }

        return Ok(VariableInfo::Compound { subfields });
    }

    // ---- Fallback 1: No BSV structure → simple Bits(width) ----
    if let Some(_width) = variable.num_bits {
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
