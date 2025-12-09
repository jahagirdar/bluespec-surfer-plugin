// =========================================================================
// lib.rs: Extism Entry Points and Dependencies
// =========================================================================

// =========================================================================
// src/ingest.rs (Revised)
// =========================================================================


// --- Import items from helper module (The "Headers") ---
use crate::helper::{
    // Static variables
    BSV_MAPS, BSV_MODULES, BSV_TYPEDEFS, BSV_LOOKUP,
    // Data structures
    RawBlockPort, TypeSegment, TypeStructure, TypeCategory, RawBlockDefinition, ModuleData,
};

// ... rest of ingest.rs functions ...
// Now `RawBlockPort`, `TypeSegment`, `TypeStructure`, `TypeCategory`,
// `RawBlockDefinition`, `ModuleData`, and the `BSV_` statics should resolve.
use extism_pdk::{ warn};
use serde::Deserialize;
use std::collections::HashMap;
use serde_json::Value; 


// macro_rules! lock {
//     ($mutex:expr) => {
//         match $mutex.lock() {
//             Ok(guard) => guard,
//             Err(poisoned) => {
//                 extism_pdk::warn!("Mutex poisoned, recovering: {:?}", poisoned);
//                 poisoned.into_inner()
//             }
//         }
//     };
// }
use extism_pdk::host_fn;
#[host_fn]
extern "ExtismHost" {
    pub fn read_file(filename: String) -> Vec<u8>;
    pub fn file_exists(filename: String) -> bool;
}
// -------------------------------------------------------------------------
// ingest.rs: JSON Processing and Initialization
// -------------------------------------------------------------------------

// --- JSON Deserialization Structs ---

#[derive(Deserialize, Debug, Clone)] 
struct RawSegment {
    #[serde(rename = "var")]
    var_name: Option<String>,
    #[serde(rename = "type")]
    type_name: String,
    min: isize,
    max: isize,
    width: usize,
}

#[derive(Deserialize, Debug)]
struct RawEnumMember {
    #[serde(rename = "value")] 
    tag: u64,
    #[serde(rename = "name")]
    name: String,
}

#[derive(Deserialize, Debug)]
struct ModuleBlockJson {
    #[serde(rename = "type")]
    type_name: String,
    ports: Vec<RawBlockPort>,
}

#[derive(Deserialize, Debug)]
struct ModuleContent {
    typedefs: HashMap<String, Value>, 
    blocks: HashMap<String, ModuleBlockJson>, 
}

#[derive(Deserialize, Debug)]
struct DesignFile {
    #[serde(rename = "top")]
    _top: String,
    #[serde(flatten)]
    modules: HashMap<String, ModuleContent>, 
}

#[derive(Deserialize, Debug)]
struct ModuleMapping(HashMap<String, Vec<String>>);
#[derive(Deserialize, Debug)]
struct ModuleMapContent {
    #[serde(flatten)]
    maps: HashMap<String, ModuleMapping>,
}


// --- Data Ingestion Functions ---

fn read_bsv_file(filename: &str) -> Vec<u8> {
    unsafe {
        read_file(filename.to_string()).unwrap_or_default()
    }
}

fn process_nested_segments(raw_segments: Vec<RawSegment>) -> Result<Vec<TypeSegment>, Box<dyn std::error::Error>> {
    let mut groups: HashMap<String, Vec<RawSegment>> = HashMap::new();
    let mut top_level_names: Vec<String> = Vec::new();

    for seg in raw_segments {
        let var_name = seg.var_name.clone().unwrap_or_default();
        let parts: Vec<&str> = var_name.splitn(2, '.').collect();
        let top_name = parts[0].to_string();

        if !top_level_names.contains(&top_name) {
            top_level_names.push(top_name.clone());
        }

        groups.entry(top_name).or_default().push(seg);
    }

    let mut final_segments: Vec<TypeSegment> = Vec::new();

    for top_name in top_level_names {
        if let Some( group) = groups.remove(&top_name) {

            let is_nested = group.iter().any(|s| s.var_name.as_ref().map_or(false, |n| n.contains('.')));

            if is_nested {
                // --- NESTED CASE: Recurse and create compound segment ---
                let remaining_segments: Vec<RawSegment> = group.into_iter().map(|mut seg| {
                    if let Some(name) = seg.var_name.as_mut() {
                        if let Some(dot_index) = name.find('.') {
                            *name = name[(dot_index + 1)..].to_string();
                        }
                    }
                    seg
                }).collect();

                let inner_segments = process_nested_segments(remaining_segments)?;

                // Calculate bounds from ACTUAL inner segments
                let inner_max_msb = inner_segments.iter().map(|s| s.msb).max().unwrap_or(0);
                let inner_min_lsb = inner_segments.iter().map(|s| s.lsb).min().unwrap_or(0);
                let inner_total_width = inner_max_msb.saturating_add(1);

                let inner_structure = TypeStructure {
                    total_width: inner_total_width,
                    segments: inner_segments,
                    enum_definition: None,
                };

                final_segments.push(TypeSegment {
                    name: Some(top_name),
                    msb: inner_max_msb,
                    lsb: inner_min_lsb,
                    type_name: "Compound".to_string(),
                    nested_structure: Some(Box::new(inner_structure)),
                });

            } else {
                // --- LEAF CASE: Process ALL segments, not just the first ---
                for seg in group {
                    final_segments.push(TypeSegment {
                        name: seg.var_name,
                        msb: seg.max.abs() as usize,
                        lsb: seg.min.abs() as usize,
                        type_name: seg.type_name,
                        nested_structure: None,
                    });
                }
            }
        }
    }

    Ok(final_segments)
}

// src/ingest.rs (Corrected process_typedef)

fn process_typedef(type_name: &str, raw_value_ref: Value, bsv_typedefs: &mut HashMap<String, TypeStructure>, bsv_lookup: &mut HashMap<String, TypeCategory>) -> Result<(), Box<dyn std::error::Error>> {

    if type_name.starts_with("Bit#(") || type_name == "Bool" || type_name == "Clock" || type_name == "Reset" {
        warn!("INGEST: Explicitly marking primitive type '{}' as Bits.", type_name);
        bsv_lookup.insert(type_name.to_string(), TypeCategory::Bits);
        return Ok(());
    }
    if let Ok(raw_segments) = serde_json::from_value::<Vec<RawSegment>>(raw_value_ref.clone()) {
        if !raw_segments.is_empty() {
            let segments = process_nested_segments(raw_segments)?;
            let total_width = segments.iter().map(|s| s.msb).max().unwrap_or(0).saturating_add(1);

            if segments.is_empty() {
                return Err(format!("No valid segments for typedef {}", type_name).into());
            }

            // NOTE: The TypeStructure constructor here should be updated to include
            // `enum_definition: None` since this is a Struct/Compound type.
            bsv_typedefs.insert(type_name.to_string(), TypeStructure {
                total_width,
                segments,
                enum_definition: None // Assumes the new field is present
            });

            bsv_lookup.insert(type_name.to_string(), TypeCategory::Struct);
            return Ok(());
        }
    }


    // Attempt to parse as Enum Members
    if let Ok(raw_members) = serde_json::from_value::<Vec<RawEnumMember>>(raw_value_ref.clone()) {
        if !raw_members.is_empty() {
            let max_val = raw_members.iter().map(|m| m.tag).max().unwrap_or(0);
            let total_width = if max_val > 0 {
                (max_val as f64).log2().ceil() as usize
            } else {
                1 // Minimum width is 1 bit
            };

            let mut enum_members = HashMap::new();
            for member in raw_members {
                enum_members.insert(member.tag, member.name);
            }

            // 🌟 FIX: Create the single segment for the enum
            let segment = TypeSegment {
                name: None,
                msb: total_width.saturating_sub(1), // Correct MSB (width - 1)
                lsb: 0,
                // 🌟 FIX: Store the canonical type name here.
                type_name: type_name.to_string(),
                nested_structure: None
            };

            // 🌟 FIX: Store the TypeStructure with the enum definition members correctly
            // placed in the dedicated `enum_definition` field.
            bsv_typedefs.insert(type_name.to_string(), TypeStructure {
                total_width,
                segments: vec![segment],
                enum_definition: Some(crate::helper::EnumDefinition { members: enum_members }), // Use new field
            });

            bsv_lookup.insert(type_name.to_string(), TypeCategory::Enum);
            return Ok(());
        }
    }

    // Default or empty definition is Bits/other simple type
    if type_name.starts_with("Bit#") || type_name == "Bool" || raw_value_ref.is_array() && raw_value_ref.as_array().unwrap().is_empty() {
        bsv_lookup.insert(type_name.to_string(), TypeCategory::Bits);
    } else {
        // Fallback for types that weren't parsed but exist
        bsv_lookup.insert(type_name.to_string(), TypeCategory::Bits);
    }

    Ok(())
}

fn process_module_blocks(
    raw_blocks: HashMap<String, ModuleBlockJson> 
) -> Result<HashMap<String, RawBlockDefinition>, Box<dyn std::error::Error>> {

    let mut block_defs_map: HashMap<String, RawBlockDefinition> = HashMap::new();

    for (instance_name, block) in raw_blocks {
        block_defs_map.insert(instance_name, RawBlockDefinition {
            block_type_name: block.type_name,
            ports: block.ports,
        }); 
    }

    Ok(block_defs_map)
}

pub fn initialize_static_data() -> Result<(), Box<dyn std::error::Error>> {
    let bsv_file_bytes = read_bsv_file("bluespec.json");
    let map_file_bytes = read_bsv_file("bluespec_map.json");
    
    if bsv_file_bytes.is_empty() { return Err(Box::<dyn std::error::Error>::from("Failed to read bluespec.json")); }
    if map_file_bytes.is_empty() { warn!("Warning: Failed to read bluespec_map.json. Scope path resolution may be limited."); }

    let file_content: DesignFile = serde_json::from_slice(&bsv_file_bytes)?; 
    let map_content: ModuleMapContent = serde_json::from_slice(&map_file_bytes)?;

    let mut bsv_modules_map = HashMap::new();
    let mut bsv_typedefs = HashMap::new(); 
    let mut bsv_lookup = HashMap::new(); 

    // --- Process Typedefs and Blocks ---
    for (module_name, module_content) in file_content.modules {
        for (type_name, raw_value_ref) in module_content.typedefs.iter() {
            if let Err(e) = process_typedef(type_name, raw_value_ref.clone(), &mut bsv_typedefs, &mut bsv_lookup) {
                warn!("Error processing typedef '{}': {}", type_name, e);
            }
        }
        
        let module_blocks = process_module_blocks(module_content.blocks)?; 
        
        bsv_modules_map.insert(module_name, ModuleData { blocks: module_blocks });
    }
    warn!("bsv_typedefs {:?}",bsv_typedefs);
    warn!("bsv_lookup {:?}",bsv_lookup);
    warn!("bsv_modules {:?}",bsv_modules_map);
    
    // --- Assign Static Globals ---
//   *BSV_TYPEDEFS.lock().unwrap() = bsv_typedefs; 
//   *BSV_LOOKUP.lock().unwrap() = bsv_lookup;
//   *BSV_MODULES.lock().unwrap() = bsv_modules_map;
    
    // In src/ingest.rs (replace the assignment block at the end of the file)

    // --- Assign Static Globals (Safe version) ---
    let mut type_g= BSV_TYPEDEFS.write().unwrap() ;
    *type_g = bsv_typedefs;
    let mut lookup_g=BSV_LOOKUP.write().unwrap();
    *lookup_g = bsv_lookup;
    let mut mod_g = BSV_MODULES.write().unwrap();
    *mod_g = bsv_modules_map;
    // --- Process Maps ---
    let mut maps_guard = BSV_MAPS.write().unwrap();
    *maps_guard = map_content.maps.into_iter().map(|(k, ModuleMapping(v))| (k, v)).collect();

    Ok(())
}

