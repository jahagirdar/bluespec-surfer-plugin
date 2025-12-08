// =========================================================================
// lib.rs: Extism Entry Points and Dependencies
// =========================================================================

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
// Declares that Cargo should look for helper code in src/helper.rs
mod helper;
// Declares that Cargo should look for ingestion code in src/ingest.rs
mod ingest;
// Declares that Cargo should look for translation logic in src/translators.rs
mod translators;


// --- 3. Public Re-exports (Making sub-module items available to *this* module) ---

// Re-export the static state definitions and core data types from helper/ingest
pub use helper::*;
pub use ingest::initialize_static_data;
pub use translators::*;

// You may need to explicitly import the functions you need from the new files
// E.g., for use in the translate/variable_info plugin_fns:
use helper::{get_variable_type_name};
use translators::{get_struct_fields_info, translate_enum, translate_recursive};

// --- Host Functions ---

use extism_pdk::host_fn;
#[host_fn]
extern "ExtismHost" {
    pub fn read_file(filename: String) -> Vec<u8>;
    pub fn file_exists(filename: String) -> bool;
}

// --- Static Global State (As requested) ---

// -------------------------------------------------------------------------
// --- Extism Plugin Functions ---
// -------------------------------------------------------------------------

#[plugin_fn]
pub fn name() -> FnResult<String> { Ok("Bluespec Translator".to_string()) }

#[plugin_fn]
pub fn new() -> FnResult<()> {
    initialize_static_data().map_err(|e| Error::msg(e.to_string()).into())
}

#[plugin_fn]
pub fn translates(variable: VariableMeta<(), ()>) -> FnResult<TranslationPreference> {
    warn!("translates");
    // Optimization: Only translate if we can resolve a block and type.
    if get_variable_type_name(&variable).is_some() {
        return Ok(TranslationPreference::Prefer);
    }
    Ok(TranslationPreference::No)
}

#[plugin_fn]
pub fn translate(params: Json<TranslateParams>) -> FnResult<TranslationResult> {
    warn!("translate");
    let variable = &params.0.variable;
    let value = &params.0.value;
    
    // 1. Determine the Type Name using the complex resolution logic
    let type_name = get_variable_type_name(variable)
        .ok_or_else(|| Error::msg(format!("Failed to determine type for variable: {}", variable.var.name)))?;

    // 2. Resolve Type Category
    let lookup_guard = BSV_LOOKUP.lock().unwrap();
    let type_category = lookup_guard.get(&type_name).unwrap_or(&TypeCategory::Bits);
    
    let digits_str = match value {
        VariableValue::BigUint(b) => format!("{:b}", b),
        VariableValue::String(s) => s.clone(),
    };
    
    let width = variable.num_bits.unwrap_or(digits_str.len() as u32) as usize;
    let padding = width.saturating_sub(digits_str.len());
    let padded_digits: String = std::iter::repeat('0').take(padding).chain(digits_str.chars()).collect();
    let digits_vec: Vec<char> = padded_digits.chars().collect();
    warn!("translate variables set");
    
    // 3. Dispatch to Translator based on Category
    match type_category {
        TypeCategory::Struct => {
            let typedefs_guard = BSV_TYPEDEFS.lock().unwrap();
            
            let struct_def = typedefs_guard.get(&type_name)
                .ok_or_else(|| Error::msg(format!("Struct definition missing for: {}", type_name)))?;
            
            warn!("translate: width={}, digits_len={}, struct_segments={}",
      width, digits_vec.len(), struct_def.segments.len());
warn!("translate: struct_total_width={}", struct_def.total_width);  // This will show the invalid value
    warn!("translate translate_recursive called");
    let tr=translate_recursive(struct_def, width, &digits_vec);
    warn!("translate translate_recursive Done");
            Ok(tr)
        }
        TypeCategory::Enum => {
    warn!("translate enum called");
            Ok(translate_enum(&type_name, width, &padded_digits))
        }
        _ => {
            // Default to Bits (handles Bits and other unrecognized types)
            Ok(TranslationResult {
                val: surfer_translation_types::ValueRepr::Bits(width as u64, padded_digits),
                subfields: vec![],
                kind: ValueKind::Normal,
            })
        }
    }
}

// *** CORRECTLY IMPLEMENTED variable_info ***
#[plugin_fn]
pub fn variable_info(variable: VariableMeta<(), ()>) -> FnResult<VariableInfo> {
    
    warn!("variable_info");
    let type_name = get_variable_type_name(&variable)
        .ok_or_else(|| Error::msg(format!("Failed to determine type for variable: {}", variable.var.name)))?;

    let bsv_lookup = BSV_LOOKUP.lock().unwrap();
    let type_category = bsv_lookup.get(&type_name).unwrap_or(&TypeCategory::Bits);
    
    match type_category {
        // Mapped to String because the provided VariableInfo enum lacks an Enum variant.
        TypeCategory::Enum => Ok(VariableInfo::String), 
        
        TypeCategory::Struct => {
            let bsv_typedefs = BSV_TYPEDEFS.lock().unwrap();
            
            let struct_def = bsv_typedefs.get(&type_name)
                .ok_or_else(|| Error::msg(format!("Struct definition missing for: {}", type_name)))?;
            
            // Call the recursive helper function
            Ok(get_struct_fields_info(struct_def, &bsv_lookup, &bsv_typedefs))
        }
        _ => Ok(VariableInfo::Bits),
    }
}

#[plugin_fn]
pub fn variable_name_info(_variable: Json<VariableMeta<(), ()>>) -> FnResult<Option<VariableNameInfo>> {
    warn!("variable_name_info");
    Ok(None)
}



