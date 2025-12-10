import json
import re 
import sys
from collections import defaultdict
from pyparsing import (
    Word,
    Literal,
    Suppress,
    alphas,
    alphanums,
    Group,
    Optional,
    Char,
    delimitedList,
    ZeroOrMore
)

# --- Configuration ---
LOG_FILE = "dsyminitial.log"
JSON_FILE = "bluespec.json"
# ---------------------

# --- Pyparsing Grammar Definitions ---

# Shared Tokens
identifier = Word(alphas, alphanums + "_" + ".")
integer = Word("0123456789")

open_paren = Suppress(Literal("("))
close_paren = Suppress(Literal(")"))
open_bracket = Suppress(Literal("["))
close_bracket = Suppress(Literal("]"))
comma = Suppress(Literal(","))
star = Literal("*")
pound = Literal("#")

# 1. TIdata (enum) Grammar for Type Identification (Pass 1)
# -----------------------------------------------------------
type_char = Char("$#*")
type_arrow = Suppress(Literal("->"))
type_arg = identifier | type_char | type_arrow | star | pound
complex_type_signature = open_paren + ZeroOrMore(type_arg) + close_paren
type_info_content = (star | complex_type_signature | identifier)

alu_op_list = Group(delimitedList(identifier))
tidata_enum_structure = (
    Suppress(Literal("TIdata"))
    + open_paren
    + Suppress(Literal("enum"))
    + close_paren
    + open_bracket
    + alu_op_list
    + close_bracket
)

ti_data_structure = (
    open_paren
    + identifier("type_name")
    + comma
    + Suppress(Literal("TypeInfo"))
    + type_info_content * (1, 3)
    + open_paren
    + tidata_enum_structure
    + close_paren
    + Optional(comma)
    + close_paren
)

# 2. ConInfo Grammar for Tag Extraction (Pass 2)
# -----------------------------------------------
colon_greater_colon = Suppress(Literal(":>:"))
arrow = Suppress(Literal("->"))
colon_colon = Suppress(Literal("::"))
equal_sign = Suppress(Literal("="))

op_mapping = (
    identifier("op_symbol") 
    + colon_greater_colon
    + open_paren + Suppress(Optional(identifier)) + close_paren
    + arrow
    + identifier("type_name_check")
)

tag_info = (
    integer + Suppress(Literal("of")) + integer
    + comma
    + Suppress(Literal("tag")) + equal_sign
    + integer("tag_value")
    + colon_colon
    + Suppress(Literal("Bit")) + integer
)

coninfo_structure = (
    open_bracket
    + Suppress(Literal("ConInfo"))
    + identifier("enum_type_name")
    + open_paren + Suppress(Literal("visible")) + close_paren
    + open_paren
    + op_mapping
    + close_paren
    + open_paren
    + tag_info
    + close_paren
    + close_bracket
)

# --- Core Functions ---

def load_log_content(log_filepath):
    """Loads and returns the content of the log file."""
    try:
        with open(log_filepath, 'r') as f:
            return f.read()
    except FileNotFoundError:
        print(f"Error: Log file not found at {log_filepath}")
        sys.exit(1)

def identify_enum_types(log_content):
    """
    Pass 1: Identifies all enum type names using the TIdata (enum) pyparsing grammar.
    Returns a set of fully qualified enum names (e.g., 'test1.Colors_e').
    """
    print("🔍 Pass 1: Identifying enum type names...")
    
    enum_types = set()
    
    for tokens, _, _ in ti_data_structure.scanString(log_content):
        enum_types.add(tokens["type_name"])
    
    print(f"✅ Found {len(enum_types)} potential enum type(s).")
    return enum_types

def extract_enum_members_and_tags(log_content, confirmed_enum_types_dot):
    """
    Pass 2: Extracts member names and tag values using the ConInfo pyparsing grammar.
    Returns a dict: { 'test1::Colors_e': [{'name': 'Red', 'value': 1}, ...] }
    """
    print("\n🔍 Pass 2: Extracting member tags and formatting...")
    
    raw_enums = defaultdict(dict)
    
    for tokens, _, _ in coninfo_structure.scanString(log_content):
        enum_type_name_dot = tokens["enum_type_name"]
        
        if enum_type_name_dot in confirmed_enum_types_dot:
            member_symbol = tokens["op_symbol"]
            tag_value = int(tokens["tag_value"])
            
            display_name = member_symbol.split('.')[-1]
            raw_enums[enum_type_name_dot][display_name] = tag_value
    
    final_enum_data = {}
    for enum_type_name_dot, members in raw_enums.items():
        json_list = []
        for name, value in sorted(members.items(), key=lambda item: item[1]):
            json_list.append({
                "name": name,
                "value": value
            })
        
        json_key = enum_type_name_dot.replace('.', '::')
        final_enum_data[json_key] = json_list
        
    return final_enum_data

def get_used_types_in_module(module_data):
    """
    Collects all unique type names referenced within a module's typedefs (structs) 
    and blocks (ports).
    """
    used_types = set()
    
    if "typedefs" in module_data:
        for type_name, definition in module_data["typedefs"].items():
            if isinstance(definition, list) and all(isinstance(item, dict) for item in definition):
                if not definition or 'var' in definition[0]:
                    for segment in definition:
                        if 'type' in segment and segment['type']:
                            used_types.add(segment['type'])
            used_types.add(type_name)
    
    if "blocks" in module_data:
        for block_info in module_data["blocks"].values():
            if "ports" in block_info:
                for port in block_info["ports"]:
                    if 'type' in port and port['type']:
                        used_types.add(port['type'])
                        
    return used_types

def handle_union(hsh):
    for module in hsh:
        print(module,type(hsh[module]))
        if isinstance(hsh[module],str):
             continue
        print(hsh[module].keys())
        if 'typedefs' in hsh[module]:
            print("has typedefs")
            for ty,val in hsh[module]["typedefs"].items():
                for v in val:
                    if 'type' in v:
                        if ty == v['type']:
                            print(f"fixing {ty} {v}")
                            v['type']=f'Bit#({v["width"]})'
def update_bluespec_json(json_filepath, all_enum_json_data):
    """
    Loads bluespec.json, determines which modules use which enums, and 
    INSERTS OR OVERWRITES them with the correct definitions (guaranteed replacement).
    """
    print(f"\n💾 Updating {json_filepath} based on module usage (and ensuring replacement)...")
    
    try:
        with open(json_filepath, 'r') as f:
            data = json.load(f)
    except Exception as e:
        print(f"Error handling JSON file: {e}")
        sys.exit(1)
        
    modules_to_process = [k for k in data.keys() 
                          if isinstance(data[k], dict) and 'typedefs' in data[k]]
    
    total_inserted_or_replaced_count = 0
    
    for module_name in modules_to_process:
        module_data = data[module_name]
        used_types = get_used_types_in_module(module_data)
        
        for enum_key, enum_definition in all_enum_json_data.items():
            if enum_key in used_types:
                
                action = "Inserted"
                if enum_key in module_data["typedefs"]:
                    # Key Change: Overwrite logic is implemented by the unconditional assignment below
                    action = "Replaced (Was wrong)"
                
                # --- Unconditional Assignment (The key change) ---
                module_data["typedefs"][enum_key] = enum_definition
                total_inserted_or_replaced_count += 1
                print(f"   {action} '{enum_key}' into '{module_name}'.")
    
    if total_inserted_or_replaced_count == 0:
         print("⚠️ No new enum definitions were inserted or replaced. All found enums were either already correct or not referenced in the module usage.")
    
    handle_union(data)
    with open(json_filepath, 'w') as f:
        json.dump(data, f, indent=4)
        print(f"\n✅ Changes saved to {json_filepath}.")

def main():
    # 0. Load content
    log_content = load_log_content(LOG_FILE)
    
    # 1. Identify all enum types (Pass 1)
    confirmed_enum_types_dot = identify_enum_types(log_content)
    
    # 2. Extract members and tags, convert to JSON format (Pass 2)
    all_enum_json_data = extract_enum_members_and_tags(log_content, confirmed_enum_types_dot)
    
    # 3. Determine module usage and insert/overwrite in bluespec.json
    update_bluespec_json(JSON_FILE, all_enum_json_data)

if __name__ == "__main__":
    main()
