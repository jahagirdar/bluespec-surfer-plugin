// =========================================================================
// src/translators.rs (Revised)
// =========================================================================

use extism_pdk::{debug};
use std::collections::HashMap; // <--- ADDED

// Import necessary types from external crate
use surfer_translation_types::{
    SubFieldTranslationResult, TranslationResult, ValueKind, VariableInfo,
};

// --- Import items from helper module (The "Headers") ---
use crate::helper::{
    TypeSegment,TypeCategory, TypeStructure, BSV_LOOKUP, BSV_TYPEDEFS,BSVTypedefsGuard,
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


// =========================================================================
// src/translators.rs (REVISED)
// =========================================================================

//use num_traits::ToPrimitive; // Assuming this is imported or in scope for u64 conversion

// Import necessary types from external crate


// =========================================================================
// Helper Function: Enum Translation
// =========================================================================





// Import necessary types
use surfer_translation_types::{
ValueRepr,
};

// --- Import items from helper module ---






// ... other imports ...

/// Translates an enum value using a pre-loaded definition (lock-safe).
/// The signature accepts the pre-cloned/pre-loaded EnumDefinition structure.
// src/translators.rs (or wherever translate_enum is defined)

// Note: This function is assumed to be called with the canonical type name
pub fn translate_enum(type_name: &str, _width: usize, digits: &str) -> TranslationResult {
    debug!("translate_enum: called with type_name {:?}, _width {:?}, digits {:?}", type_name, _width, digits);
    let typedefs_guard = BSV_TYPEDEFS.read().unwrap();
    let struct_def = typedefs_guard.get(type_name);

    let members: HashMap<u64, String> = if let Some(def) = struct_def {
        // 🌟 FIX: Read members from the new `enum_definition` field
        def.enum_definition.as_ref()
            .map(|e| e.members.clone())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };

    let val = u64::from_str_radix(digits, 2).unwrap_or(0);
    let name = members.get(&val).cloned().unwrap_or_else(|| format!("Unknown({})", val));

    // ... rest of the function remains the same ...
    TranslationResult {
        val: surfer_translation_types::ValueRepr::String(name),
        subfields: vec![],
        kind: ValueKind::Normal,
    }
}

/// Recursively translates a segment of the bitstring based on the struct definition.
// =========================================================================
// src/translators.rs::translate_recursive (Fixed for Nested Structures)
// =========================================================================



// ... (translate_enum function is unchanged) ...


/// Recursively translates a segment of the bitstring based on the struct definition.
// src/translators.rs

pub fn translate_data_by_category(
    segment: &TypeSegment,
    type_category: &TypeCategory,
    chunk_slice: &[char],
    segment_width: usize,
    bsv_typedefs_guard: &BSVTypedefsGuard,
) -> TranslationResult {

    let chunk_str: String = chunk_slice.iter().collect();

    match type_category {
        // Case 1: ENUM
        TypeCategory::Enum => {
            let enum_result = translate_enum(&segment.type_name, segment_width, &chunk_str);

            // Check if translate_enum failed (assumed: it returns an empty string/unknown tag)
            let is_failure = matches!(&enum_result.val, ValueRepr::String(s) if s.is_empty());

            if is_failure {
                // Requirement: Return Error Kind with value "Error" idx 0
                debug!("ENUM FAIL: No tag match found for {} in segment '{}'", segment.type_name,
                      segment.name.clone().unwrap_or_default());
                TranslationResult {
                    val: ValueRepr::String("Error".to_string()),
                    subfields: vec![],
                    kind: ValueKind::Error, // Assuming Kind::Error(0) for no match
                }
            } else {
                enum_result
            }
        }

        // Case 2: BITS
        TypeCategory::Bits => {
            // Requirement: If data is Bits return Bits
            TranslationResult {
                val: ValueRepr::Bits(segment_width as u64, chunk_str),
                subfields: vec![],
                kind: ValueKind::Normal,
            }
        }

        // Case 3: COMPOUND (Struct/Union)
        TypeCategory::Struct => {
            // Requirement: If data is Compound call translate_compound
            translate_compound(
                segment,
                chunk_slice,
                segment_width,
                bsv_typedefs_guard,
            )
        }

        // Default Fallback
        _ => {
            // Defaulting unhandled types (like arrays, or unknown) to Bits
            debug!("Unhandled TypeCategory {:?} for {}. Falling back to Bits.",
                  type_category, segment.type_name);
            TranslationResult {
                val: ValueRepr::Bits(segment_width as u64, chunk_str),
                subfields: vec![],
                kind: ValueKind::Normal,
            }
        }
    }
}
// src/translators.rs

pub fn translate_compound(
    segment: &TypeSegment,
    chunk_slice: &[char],
    segment_width: usize,
    bsv_typedefs_guard: &BSVTypedefsGuard,
) -> TranslationResult {

    // 1. Lookup the structure definition by name
    if let Some(struct_def) = bsv_typedefs_guard.get(&segment.type_name) {
        // Requirement: For each segment call translate_recursive()
        debug!("TR-RECURSE: Entering Compound struct '{}'", segment.type_name);
        translate_recursive(struct_def, segment_width, chunk_slice)
    } else {
        // Compound type category found, but definition is missing -> Fallback
        debug!("TR-FAIL: Struct category found for '{}', but typedef is missing. Falling back to Bits.",
              segment.type_name);

        let chunk_str: String = chunk_slice.iter().collect();
        TranslationResult {
            val: ValueRepr::Bits(segment_width as u64, chunk_str),
            subfields: vec![],
            kind: ValueKind::Normal,
        }
    }
}
// src/translators.rs

pub fn translate_recursive(
    structure: &TypeStructure,
    total_width: usize,
    digits: &[char],
) -> TranslationResult {

    // 1. Handle top-level Enum translation (Immediate exit for non-struct types)
    if structure.enum_definition.is_some() {
        if let Some(segment) = structure.segments.first() {
            let chunk_str: String = digits.iter().collect();
            return translate_enum(&segment.type_name, total_width, &chunk_str);
        }
    }

    // 2. Acquire Locks
    let bsv_lookup_guard = BSV_LOOKUP.read().unwrap();
    let bsv_typedefs_guard = BSV_TYPEDEFS.read().unwrap();

    let mut subfields = Vec::new();
    let mut processed_width =0;

    // 3. Iterate and Process Segments
    for segment in structure.segments.iter() {
        let segment_width = (segment.msb + 1).saturating_sub(segment.lsb);

        // 3a. Check boundaries and extract chunk (using the helper from the prior refactoring)
        let Some((chunk_slice, new_processed_width)) = extract_bit_chunk(
            digits, segment_width, processed_width
        ) else {
            break;
        };
        processed_width = new_processed_width;

        let name = segment.name.clone().unwrap_or_else(|| "unnamed".to_string());

        // 3b. Delegate Translation Logic
        let result =
            // 🌟 PRIORITY 1: Inlined Nested Structure (e.g., Compound type without a name lookup)
            if let Some(ref nested_struct_def) = segment.nested_structure {
                debug!("TR-RECURSE: Recursing into inlined nested structure for field '{}'", name);
                translate_recursive(nested_struct_def, segment_width, chunk_slice)

            // PRIORITY 2: Global Type Category Lookup
            } else if let Some(type_category) = bsv_lookup_guard.get(&segment.type_name) {
                // 🌟 NEW DELEGATION: Call the category-based dispatcher
                translate_data_by_category(
                    segment,
                    type_category,
                    chunk_slice,
                    segment_width,
                    &bsv_typedefs_guard,
                )
            }
            // PRIORITY 3: Unknown Type Name -> Fallback to Bits
            else {
                debug!("TR-WARN: Unknown type '{}', falling back to Bits#({})",
                      segment.type_name, segment_width);
                let chunk_str: String = chunk_slice.iter().collect();
                TranslationResult {
                    val: ValueRepr::Bits(segment_width as u64, chunk_str),
                    subfields: vec![],
                    kind: ValueKind::Normal,
                }
            };

        // 3c. Collect Result
        subfields.push(SubFieldTranslationResult { name, result: result });
    }

    // 4. Final Return (Struct assembly)
    TranslationResult {
        val: ValueRepr::Struct,
        subfields,
        kind: ValueKind::Normal,
    }
}

fn extract_bit_chunk<'a>(
    digits: &'a [char],
    segment_width: usize,
    processed_width: usize,
) -> Option<(&'a [char], usize)> {

    // In an MSB-first array, the start index is simply the width processed so far.
    let start_idx = processed_width;

    // The end index is the start plus the width of the current segment.
    let end_idx = start_idx + segment_width;

    if end_idx > digits.len() {
        // ... (boundary check and debuging) ...
        return None;
    }

    let chunk_slice = &digits[start_idx..end_idx];

    // Return the slice and the new total processed width
    Some((chunk_slice, end_idx))
}
