// Copyright: Copyright (c) 2025 Dyumnin Semiconductors. All rights reserved.
// Author: Vijayvithal <jahagirdar.vs@gmail.com>
// Created on: 2025-12-12
// Helper scripts: Dumping spot for all low level functions.
//
// =========================================================================
// src/helper.rs (Revised)
// =========================================================================
use extism_pdk::{debug};
use once_cell::sync::Lazy;     // <--- ADDED
use std::sync::RwLock;          // <--- ADDED
use std::sync::RwLockReadGuard;          // <--- ADDED
                               //

// --- Expose Data Structures (Must be pub) ---

// Maps original scope path to implementation module names: {top: {mkTop: [main, top]}}
pub static BSV_MAPS: Lazy<RwLock<HashMap<String, HashMap<String, Vec<String>>>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Stores processed module information for block lookup: {mkTop: {blocks: {rb: RawBlockDefinition, ...}}}
pub static BSV_MODULES: Lazy<RwLock<HashMap<String, ModuleData>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Unflattened and unique type definitions: {test1::Bar_st: TypeStructure}
pub static BSV_TYPEDEFS: Lazy<RwLock<HashMap<String, TypeStructure>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

// Lookup table for type category: {test1::Colors_e: Enum, test1::Bar_st: Struct}
pub static BSV_LOOKUP: Lazy<RwLock<HashMap<String, TypeCategory>>> = Lazy::new(|| {
    RwLock::new(HashMap::new())
});

pub type BSVTypedefsGuard<'a> = RwLockReadGuard<'a, HashMap<String, TypeStructure>>;
pub type BSVLookupGuard<'a> = RwLockReadGuard<'a, HashMap<String, TypeCategory>>;
// macro_rules! lock {
//     ($mutex:expr) => {
//         match $mutex.lock() {
//             Ok(guard) => guard,
//             Err(poisoned) => {
//                 extism_pdk::debug!("Mutex poisoned, recovering: {:?}", poisoned);
//                 poisoned.into_inner()
//             }
//         }
//     };
// }

// --- Utility Functions (Must be pub if used by other modules) ---
use serde::Deserialize;
use std::collections::HashMap;
use regex::Regex;

use surfer_translation_types::{
     TranslationResult,
    VariableMeta,
    // Removed StructInfo and FieldInfo imports (E0432) as VariableInfo::Compound is expected.
};
// -------------------------------------------------------------------------
// helper.rs: Data Structures and Utilities
// -------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum TypeCategory {
    Bits,
    Enum,
    Bool,
    Struct,
    Union,
    Interface,
    // Add more types here as needed
}

#[derive(Debug, Clone)]
pub struct TypeSegment {
    pub name: Option<String>,
    pub msb: usize,
    pub lsb: usize,
    pub type_name: String,
    pub nested_structure: Option<Box<TypeStructure>>, 
}

#[derive(Debug, Clone)]
pub struct TypeStructure {
    pub total_width: usize,
    pub segments: Vec<TypeSegment>,
    pub enum_definition: Option<EnumDefinition>,
}

#[derive(Debug, Clone)]
pub struct EnumDefinition {
    pub members: HashMap<u64, String>,
}

// *** FIX E0277 & Serde Attribute Error: Added #[derive(Deserialize)] ***
#[derive(Deserialize, Debug, Clone)] 
pub struct RawBlockPort {
    #[serde(rename = "var")]
    name: String,
    #[serde(rename = "type")]
    type_name: String,
}

#[derive(Debug, Clone)]
pub struct RawBlockDefinition {
    pub block_type_name: String,
    pub ports: Vec<RawBlockPort>,
}

#[derive(Debug, Clone)]
pub struct ModuleData {
    // Maps instance name (e.g., "rb") to its full block definition (ports included)
    pub blocks: HashMap<String, RawBlockDefinition>,         
}


pub fn create_no_translation_result() -> TranslationResult {
    TranslationResult {
        val: surfer_translation_types::ValueRepr::String("".to_string()),
        subfields: vec![],
        kind: surfer_translation_types::ValueKind::Normal,
    }
}

// --- Constants for Port Priority ---
const IGNORED_PORTS: [&str; 8] = ["CLK", "RST", "EN", "CLR","FULL_N","EMPTY_N","ENQ","DEQ"];
const PREFERRED_PORTS: [&str; 7] = ["Q_OUT","D_OUT", "Probe","PROBE", "D_IN","WGET", "WHAS"];

#[derive(Debug)]
enum SignalNameFormat {
    FullVar(String),        // e.g., "rb" -> checks preferred ports of module "rb"
    PortedVar(String),      // e.g., "rb_D_OUT" -> uses type of port "D_OUT" on "rb"
    Unknown,
}

fn parse_signal_name(name: &str) -> (String, SignalNameFormat) {
    // Regex to match "BASE_NAME_PORT" or "BASE_NAME$PORT"
    let re = Regex::new(r"^(?P<base>[a-zA-Z0-9_]+)[\_$](?P<port>[a-zA-Z0-9]+)$").unwrap();

    if let Some(caps) = re.captures(name) {
        let base_name = caps.name("base").unwrap().as_str().to_string();
        let port_name = caps.name("port").unwrap().as_str().to_string();
        debug!("Matching base ={:?} port= {:?}",base_name,port_name);
        if PREFERRED_PORTS.contains(&port_name.to_uppercase().as_str()) {
            debug!("Matched {:?}",port_name);
            return (base_name, SignalNameFormat::PortedVar(port_name));
        }
    }

    // Default to FullVar
    debug!("No Matching for ={:?} ",name);
    (name.to_string(), SignalNameFormat::FullVar(name.to_string()))
}

// --- Core Variable Type Resolution Logic ---

pub fn get_current_module(scope_path: &[String]) -> Result<String, Box<dyn std::error::Error>> {
    let bsv_maps = BSV_MAPS.read().unwrap();
    let bsv_modules = BSV_MODULES.read().unwrap();

    let mut module_name: Option<String> = None;
    let mut path_start_index: usize = 0;

    for (_instance, mapping) in bsv_maps.iter() {
        for (impl_name, orig_names) in mapping.iter() {
            if scope_path.starts_with(orig_names) {
                module_name = Some(impl_name.clone());
                path_start_index = orig_names.len();
                break;
            }
        }
        if module_name.is_some() { break; }
    }

    let mut current_module = module_name.ok_or("Initial scope not found in maps")?;

    for next_instance in scope_path.iter().skip(path_start_index) {
        if let Some(module_data) = bsv_modules.get(&current_module) {
             if let Some(block_def) = module_data.blocks.get(next_instance) {
                 // Update current_module to the type name of the nested block
                 current_module = block_def.block_type_name.clone();
             }
        }
    }

    Ok(current_module)
}

pub fn get_variable_type_name(variable: &VariableMeta<(), ()>) -> Option<String> {
    let module_name = get_current_module(&variable.var.path.strs).ok()?;
    let bsv_modules = BSV_MODULES.read().unwrap();
    let module_data = bsv_modules.get(&module_name)?;

    let (instance_name, format) = parse_signal_name(variable.var.name.as_str());

    let raw_block_def = module_data.blocks.get(&instance_name)?;
    let ports = &raw_block_def.ports;
    
    match format {
        SignalNameFormat::PortedVar(port_name) => {
            // Use that specific ports type
            ports.iter()
                .find(|p| p.name == port_name)
                .map(|p| p.type_name.clone())
        }
        SignalNameFormat::FullVar(_) => {
            // Find the type of the highest priority preferred port, or first non-ignored port
            
            // a) Check preferred ports (highest priority first)
            for preferred_port in PREFERRED_PORTS.iter() {
                if let Some(port) = ports.iter().find(|p| p.name.to_uppercase() == *preferred_port) {
                    return Some(port.type_name.clone());
                }
            }
            // b) Check first non-ignored port
            ports.iter()
                .find(|p| !IGNORED_PORTS.contains(&p.name.as_str()))
                .map(|p| p.type_name.clone())
        }
        SignalNameFormat::Unknown => None,
    }
}
