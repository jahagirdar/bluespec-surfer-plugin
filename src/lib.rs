// Copyright: Copyright (c) 2025 Dyumnin Semiconductors. All rights reserved.
// Author: Vijayvithal <jahagirdar.vs@gmail.com>
// Created on: 2025-12-12
// Description: Entrypoint for wasm plugin
// =========================================================================
// lib.rs: Extism Entry Points and Dependencies
// =========================================================================

use extism_pdk::{plugin_fn, FnResult, Json, Error, debug,warn };

pub use surfer_translation_types::plugin_types::TranslateParams;
use surfer_translation_types::{
     TranslationResult,  VariableInfo,
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
// use helper::{get_variable_type_name};

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
pub fn name() -> FnResult<String> {
    Ok("Bluespec Translator".to_string())
}

#[plugin_fn]
pub fn new() -> FnResult<()> {
    extism_pdk::info!("Bluespec translator (Beta) https://github.com/jahagirdar/bluespec-surfer-plugin");
    initialize_static_data().map_err(|e| Error::msg(e.to_string()).into())
}

#[plugin_fn]
pub fn translates(variable: VariableMeta<(), ()>) -> FnResult<TranslationPreference> {
    // 1. Get the type name
    let type_name = match get_variable_type_name(&variable) {
        Some(name) => name,
        None => return Ok(TranslationPreference::No), // Can't resolve type name, so skip
    };

    // 2. Get the type category from the pre-loaded map (BSV_LOOKUP)
    let bsv_lookup_guard = BSV_LOOKUP.read().unwrap();
    let category = bsv_lookup_guard.get(&type_name);

    // 3. Dispatch based on category
    debug!("Translates: Category = {:?}",category);
    match category {
        // Types that require structural modification or symbol lookups:
        Some(TypeCategory::Struct) | Some(TypeCategory::Union) |
        Some(TypeCategory::Interface) | Some(TypeCategory::Enum) => {
            // We want to transform these complex types.
            Ok(TranslationPreference::Prefer)
        },

        // Primitives: The debugger's default display is often sufficient (raw bits),
        // and we want to avoid the extra function call overhead.
        Some(TypeCategory::Bits) | Some(TypeCategory::Bool) => {
            Ok(TranslationPreference::No)
        },

        // Unknown or unsupported types
        _ => {
            Ok(TranslationPreference::No)
        }
    }
    //Ok(translate)
}




#[plugin_fn]
// =========================================================================
// lib.rs: translate (Fixed for Slice Indexing Panic)
// =========================================================================


#[plugin_fn]
pub fn translate(params: Json<TranslateParams>) -> FnResult<TranslationResult> {
    let variable = &params.0.variable;
    let value = &params.0.value;
    debug!("translate: {:?} \n value={:?}", variable, value);

    // 1. Get type metadata (clone out of mutex)
    let type_name = get_variable_type_name(variable)
        .ok_or_else(|| Error::msg(format!("Failed to determine type for variable: {}", variable.var.name)))?;

    let struct_def = {
        let typedefs_guard = BSV_TYPEDEFS.read().unwrap();
        typedefs_guard.get(&type_name).cloned()
            .ok_or_else(|| Error::msg(format!("Struct definition missing for: {}", type_name)))?
    };

    // 2. Get VCD width and data
    let vcd_width = variable.num_bits.unwrap_or(0) as usize;
    let digits_str_unpadded = match value {
        VariableValue::BigUint(b) => format!("{:b}", b),
        VariableValue::String(s) => s.clone(),
    //   _ => return Ok(create_no_translation_result()),
    };

    // 3. 🌟 CRITICAL: Pad to TYPE width, NOT VCD width
    let type_width = struct_def.total_width; // Use the type's actual width
    let padding = type_width.saturating_sub(digits_str_unpadded.len());
    let digits_str: String = std::iter::repeat('0')
        .take(padding)
        .chain(digits_str_unpadded.chars())
        .collect();
    let digits_vec: Vec<char> = digits_str.chars().collect();

    // 4. Validate width match
    if vcd_width != type_width {
        debug!("Width mismatch: VCD={} bits, Type={} bits. Using type width.", vcd_width, type_width);
    }

    // 5. Call translator with CORRECT width
    debug!("Calling TranslateRecursive with {:?} {:?} {:?}",struct_def,type_width,digits_vec);
    let tr = translate_recursive(&struct_def, type_width, &digits_vec);
    //debug!("translate return value {:?}",tr);
    Ok(tr)
}

// *** CORRECTLY IMPLEMENTED variable_info ***
#[plugin_fn]
// In src/lib.rs (inside variable_info)

#[plugin_fn]
pub fn variable_info(variable: VariableMeta<(), ()>) -> FnResult<VariableInfo> {
    
    let type_name = get_variable_type_name(&variable)
        .ok_or_else(|| Error::msg(format!("Failed to determine type for variable: {}", variable.var.name)))?;

    let bsv_lookup=BSV_LOOKUP.read().unwrap();

    let type_category = bsv_lookup.get(&type_name).unwrap_or(&TypeCategory::Bits);
    // warn!("TypeCategoryC= {:?}",type_category);
    
    match type_category {
        // Mapped to String because the provided VariableInfo enum lacks an Enum variant.
        TypeCategory::Enum => Ok(VariableInfo::String), 
        
        TypeCategory::Struct => {
            let bsv_typedefs = BSV_TYPEDEFS.read().unwrap() ;
            
            let struct_def = bsv_typedefs.get(&type_name)
                .ok_or_else(|| Error::msg(format!("Struct definition missing for: {}", type_name)))?;
            // warn!("struct_def= {:?}",struct_def);
            
            let field_info=get_struct_fields_info(struct_def, &bsv_lookup, &bsv_typedefs);
            // Call the recursive helper function
            // warn!("field_info= {:?}",field_info);
            Ok(field_info)
        }
        TypeCategory::Bits => { if variable.num_bits == 1.into() {
            Ok(VariableInfo::Bool)}
            else {
                Ok(VariableInfo::Bits)}
        }

        _ => Ok(VariableInfo::Bits),
    }
}

#[plugin_fn]
pub fn variable_name_info(_variable: Json<VariableMeta<(), ()>>) -> FnResult<Option<VariableNameInfo>> {
    Ok(None)
}



