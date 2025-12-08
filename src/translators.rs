// =========================================================================
// src/translators.rs (Revised)
// =========================================================================

use extism_pdk::{warn};
use std::collections::HashMap; // <--- ADDED

// Import necessary types from external crate
use surfer_translation_types::{
    SubFieldTranslationResult, TranslationResult, ValueKind, VariableInfo, VariableMeta, // Added VariableMeta/Info for VariableInfo usage
};

// --- Import items from helper module (The "Headers") ---
use crate::helper::{
    TypeCategory, TypeStructure, BSV_LOOKUP, BSV_TYPEDEFS,
    create_no_translation_result, // <--- ADDED (now that it is pub in helper.rs)
};


// --- Public Translation Functions (E0603 errors fixed here by adding `pub`) ---



// -------------------------------------------------------------------------
// translators.rs: Translation Logic (includes VariableInfo helper)
// -------------------------------------------------------------------------

/// Recursively builds the VariableInfo structure for a complex struct type.
pub fn get_struct_fields_info(
    structure: &TypeStructure, 
    bsv_lookup: &HashMap<String, TypeCategory>, 
    bsv_typedefs: &HashMap<String, TypeStructure>
) -> VariableInfo {
    
    // *** FIX E0599: Changed FieldInfo/StructInfo usage to Compound variant ***
    let subfields: Vec<(String, VariableInfo)> = structure.segments.iter().map(|segment| {
        let name = segment.name.clone().unwrap_or_else(|| "unnamed".to_string());
        
        let info = if let Some(nested_structure) = &segment.nested_structure {
            // Nested structure: recursive call
            get_struct_fields_info(nested_structure, bsv_lookup, bsv_typedefs)
        } else {
            // Simple type or a non-nested struct/enum
            match bsv_lookup.get(&segment.type_name).unwrap_or(&TypeCategory::Bits) {
                TypeCategory::Struct => {
                    if let Some(struct_def) = bsv_typedefs.get(&segment.type_name) {
                        get_struct_fields_info(struct_def, bsv_lookup, bsv_typedefs)
                    } else {
                        VariableInfo::Bits
                    }
                }
                // Use String for Enums since the VariableInfo definition lacks Enum
                TypeCategory::Enum => VariableInfo::String,
                _ => VariableInfo::Bits, // Default: Bit#(N) or unhandled simple type
            }
        };

        (name, info)
    }).collect();

    VariableInfo::Compound { subfields }
}


pub fn translate_enum(type_name: &str, _width: usize, digits: &str) -> TranslationResult {
    let typedefs_guard = BSV_TYPEDEFS.lock().unwrap();
    let struct_def = typedefs_guard.get(type_name);

    let members: HashMap<u64, String> = if let Some(def) = struct_def {
        // Retrieve members from the placeholder segment type_name
        if let Some(segment) = def.segments.first() {
            serde_json::from_str(&segment.type_name).unwrap_or_default()
        } else {
            HashMap::new()
        }
    } else {
        HashMap::new()
    };
    
    let val = u64::from_str_radix(digits, 2).unwrap_or(0);
    let name = members.get(&val).cloned().unwrap_or_else(|| format!("Unknown({})", val));
    
    TranslationResult {
        val: surfer_translation_types::ValueRepr::Enum { idx: val as usize, name },
        subfields: vec![],
        kind: ValueKind::Normal,
    }
}


pub fn translate_recursive(
    structure: &TypeStructure,
    current_bit_vector_width: usize, 
    digits_slice: &[char],
) -> TranslationResult {
    
    warn!("Start TR-0");
    let mut subfields = Vec::new();

    warn!("Start TR-1");
// *** CHANGE 1: Mutex Lock Safety for BSV_LOOKUP ***
let bsv_lookup = match BSV_LOOKUP.lock() {
    Ok(guard) => guard,
    Err(e) => {
        // 🌟 ADDED WARNING FOR VERIFICATION 🌟
        extism_pdk::warn!("T-FATAL: BSV_LOOKUP mutex poisoned in translate_recursive: {}", e);
        return create_no_translation_result(); // Cannot proceed, return safe fallback
    }
};

    warn!("TR-1");
// *** CHANGE 2: Mutex Lock Safety for BSV_TYPEDEFS ***
let bsv_typedefs = match BSV_TYPEDEFS.lock() {
    Ok(guard) => guard,
    Err(e) => {
        // 🌟 ADDED WARNING FOR VERIFICATION 🌟
        extism_pdk::warn!("T-FATAL: BSV_TYPEDEFS mutex poisoned in translate_recursive: {}", e);
        return create_no_translation_result(); // Cannot proceed, return safe fallback
    }
};
    warn!("TR-1");


    for (i, segment) in structure.segments.iter().enumerate() {
        let name = segment.name.clone().unwrap_or_else(|| format!("Field_{}", i));

        warn!("translate recursive name ={:?}",name);
        
        // Calculate bit indices (MSB: most significant bit, LSB: least significant bit)
        let msb_index = current_bit_vector_width.saturating_sub(1).saturating_sub(segment.msb);
        let lsb_index = current_bit_vector_width.saturating_sub(1).saturating_sub(segment.lsb);
        let start_idx = msb_index;
        let end_idx = lsb_index.saturating_add(1);
        let segment_width = segment.msb.saturating_sub(segment.lsb).saturating_add(1);

        let is_safe_slice = start_idx < digits_slice.len() && end_idx <= digits_slice.len();

        let result: TranslationResult = if let Some(nested) = &segment.nested_structure {
             if is_safe_slice {
                translate_recursive(nested, segment_width, &digits_slice[start_idx..end_idx])
             } else {
                create_no_translation_result()
             }
        } else if is_safe_slice {
            let chunk_slice = &digits_slice[start_idx..end_idx];
            let chunk_str: String = chunk_slice.iter().collect();

            // Check segment type category
            match bsv_lookup.get(&segment.type_name).unwrap_or(&TypeCategory::Bits) {
                TypeCategory::Enum => {
                    // Nested enum translation
                    translate_enum(&segment.type_name, segment_width, &chunk_str)
                }
                TypeCategory::Struct => {
                    // Nested struct translation (this path should be rare if unflattening is correct)
                    if let Some(struct_def) = bsv_typedefs.get(&segment.type_name) {
                        translate_recursive(struct_def, segment_width, chunk_slice)
                    } else {
                         // Treat as bits if definition is missing
                        TranslationResult {
                            val: surfer_translation_types::ValueRepr::Bits(segment_width as u64, chunk_str),
                            subfields: vec![],
                            kind: ValueKind::Normal,
                        }
                    }
                }
                _ => {
                    // Bits/Default translation
                    TranslationResult {
                        val: surfer_translation_types::ValueRepr::Bits(segment_width as u64, chunk_str),
                        subfields: vec![],
                        kind: ValueKind::Normal,
                    }
                }
            }
        } else {
            create_no_translation_result()
        };
        
        subfields.push(SubFieldTranslationResult { name, result });
    }
    warn!("TR-1");

    // Crucially returns Struct to prevent host application panic.
    TranslationResult {
        val: surfer_translation_types::ValueRepr::Struct,
        subfields,
        kind: ValueKind::Normal,
    }
}

