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
use extism_pdk::{plugin_fn, FnResult, Json, Error, warn};
use serde::Deserialize;
use std::collections::HashMap;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use serde_json::Value; 
use regex::Regex;

pub use surfer_translation_types::plugin_types::TranslateParams;
use surfer_translation_types::{
    SubFieldTranslationResult, TranslationResult, ValueKind, VariableInfo,
    VariableMeta, VariableValue, TranslationPreference, 
    translator::{VariableNameInfo}, 
    // Removed StructInfo and FieldInfo imports (E0432) as VariableInfo::Compound is expected.
};

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
        if let Some(mut group) = groups.remove(&top_name) {
            
            let is_nested = group.iter().any(|s| s.var_name.as_ref().map_or(false, |n| n.contains('.')));

            if is_nested {
                // Nested segments. Recurse after stripping the top_name prefix.
                let mut min_abs = std::isize::MAX;
                let mut max_abs = 0;
                let mut total_width = 0;
                
                let remaining_segments: Vec<RawSegment> = group.into_iter().map(|mut seg| {
                    if seg.min.abs() < min_abs { min_abs = seg.min.abs(); }
                    if seg.max.abs() > max_abs { max_abs = seg.max.abs(); }
                    total_width += seg.width;
                    
                    if let Some(name) = seg.var_name.as_mut() {
                        if let Some(dot_index) = name.find('.') {
                            *name = name[(dot_index + 1)..].to_string();
                        }
                    }
                    seg
                }).collect();

                let inner_segments = process_nested_segments(remaining_segments)?;
                
                // Calculate the max_abs and min_abs of the unflattened structure
                let max_abs = inner_segments.iter().map(|s| s.msb).max().unwrap_or(0);
                let min_abs = inner_segments.iter().map(|s| s.lsb).min().unwrap_or(0);

                let inner_structure = TypeStructure {
                    total_width,
                    segments: inner_segments,
                };

                final_segments.push(TypeSegment {
                    name: Some(top_name),
                    msb: max_abs,
                    lsb: min_abs,
                    type_name: "Compound".to_string(), // Placeholder name for nested structs
                    nested_structure: Some(Box::new(inner_structure)),
                });

            } else {
                // Simple leaf segment
                if group.len() == 1 {
                    let seg = group.remove(0);
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

fn process_typedef(type_name: &str, raw_value_ref: Value, bsv_typedefs: &mut HashMap<String, TypeStructure>, bsv_lookup: &mut HashMap<String, TypeCategory>) -> Result<(), Box<dyn std::error::Error>> {
    
    // Attempt to parse as Struct Segments
    if let Ok(raw_segments) = serde_json::from_value::<Vec<RawSegment>>(raw_value_ref.clone()) {
        if !raw_segments.is_empty() {
            let segments = process_nested_segments(raw_segments)?;
            
            let total_width = segments.iter().map(|s| s.msb).max().unwrap_or(0).saturating_add(1);
            
            bsv_typedefs.insert(type_name.to_string(), TypeStructure { total_width, segments });
            bsv_lookup.insert(type_name.to_string(), TypeCategory::Struct);
            return Ok(());
        }
    } 
    
    // Attempt to parse as Enum Members
    if let Ok(raw_members) = serde_json::from_value::<Vec<RawEnumMember>>(raw_value_ref.clone()) {
        if !raw_members.is_empty() {
            // For enums, we treat them as Bit types in BSV_TYPEDEFS for lookup stability
            // but rely on BSV_LOOKUP for actual translation logic.
            let max_val = raw_members.iter().map(|m| m.tag).max().unwrap_or(0);
            let total_width = if max_val > 0 { 
                (max_val as f64).log2().ceil() as usize 
            } else { 
                0 
            };

            // Put a placeholder structure for the Enum in TYPEDEFS to store its width and members
            let mut enum_members = HashMap::new();
            for member in raw_members {
                enum_members.insert(member.tag, member.name);
            }

            // Storing the members in the TypeStructure's segments (type_name field) for retrieval
            bsv_typedefs.insert(type_name.to_string(), TypeStructure { 
                total_width, 
                segments: vec![TypeSegment {
                    name: None, msb: 0, lsb: 0, 
                    type_name: serde_json::to_string(&enum_members).unwrap_or_default(), 
                    nested_structure: None 
                }] 
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
    
    // --- Assign Static Globals ---
    *BSV_TYPEDEFS.lock().unwrap() = bsv_typedefs; 
    *BSV_LOOKUP.lock().unwrap() = bsv_lookup;
    *BSV_MODULES.lock().unwrap() = bsv_modules_map;
    
    // --- Process Maps ---
    let mut maps_guard = BSV_MAPS.lock().unwrap();
    *maps_guard = map_content.maps.into_iter().map(|(k, ModuleMapping(v))| (k, v)).collect();

    Ok(())
}

