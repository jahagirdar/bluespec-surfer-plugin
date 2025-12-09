import json
import re
import sys
from collections import defaultdict

# --- Configuration ---
LOG_FILE = "dsyminitial.log"
JSON_FILE = "bluespec.json"
# TARGET_MODULE is intentionally removed as we now iterate over all modules
# ---------------------

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
    Pass 1: Identifies all enum type names using the 'TIdata (enum)' pattern.
    Returns a set of fully qualified enum names (e.g., 'test1.Colors_e').
    """
    print("🔍 Pass 1: Identifying enum type names...")
    
    # Pattern looks for: (test1.Colors_e, TypeInfo * (TIdata (enum) [Red, Blue, Green, Black]))
    enum_type_pattern = re.compile(
        r'\(([\w.]+),\s+TypeInfo\s+.*?\s+\(TIdata\s+\(enum\).*?\)'
    )
    
    enum_types = set()
    for match in enum_type_pattern.finditer(log_content):
        type_name = match.group(1).strip()
        enum_types.add(type_name)
    
    print(f"✅ Found {len(enum_types)} potential enum type(s).")
    return enum_types

def extract_enum_members_and_tags(log_content, enum_types):
    """
    Pass 2: Extracts member names and tag values for the identified enum types.
    Returns a dict: { 'test1::Colors_e': [{'name': 'Red', 'value': 1}, ...] }
    """
    print("\n🔍 Pass 2: Extracting member tags and formatting...")
    
    # Raw data storage: { 'test1.Colors_e': { 'Red': 1, 'Blue': 20, ... } }
    raw_enums = defaultdict(dict)
    
    # Pattern finds: [ConInfo test1.Colors_e (visible) (test1.Red :>: ... tag = 1 :: Bit 6)]
    # Group 1: Full enum type name (e.g., 'test1.Colors_e').
    # Group 2: Symbolic member name (e.g., 'Red' or 'test1.Red').
    # Group 3: Integer tag value (e.g., '1').
    member_tag_pattern = re.compile(
        r'ConInfo\s+([\w.:]+)\s+.*?\((?:[\w.]+\.)?([\w.]+)\s*:>.*?\)\s*\(.*?\s*tag\s*=\s*(\d+)\s*::\s*Bit\s*(\d+)\)\s*\]',
        re.DOTALL
    )

    for match in member_tag_pattern.finditer(log_content):
        enum_type_name_dot = match.group(1).strip() # e.g., 'test1.Colors_e'
        
        # Only process if this type was confirmed as an enum in Pass 1
        if enum_type_name_dot in enum_types:
            member_symbol = match.group(2).strip()
            tag_value = int(match.group(3))
            
            # Use the simple name (e.g., 'Red') for the JSON 'name' field
            display_name = member_symbol.split('.')[-1]
            
            # Store raw data
            raw_enums[enum_type_name_dot][display_name] = tag_value
    
    # Convert raw data to the final JSON structure and use '::' separator in keys
    final_enum_data = {}
    for enum_type_name_dot, members in raw_enums.items():
        json_list = []
        # Sort by value for clean JSON output
        for name, value in sorted(members.items(), key=lambda item: item[1]):
            json_list.append({
                "name": name,
                "value": value
            })
        
        # Convert 'test1.Colors_e' to 'test1::Colors_e' for the JSON key
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
            # If the definition is a list of dictionaries, assume it's a struct definition
            # (or already an enum, which we handle below).
            if isinstance(definition, list) and all(isinstance(item, dict) for item in definition):
                if not definition or 'var' in definition[0]: # Heuristic check for struct fields
                    # This is a struct definition; collect the types of its fields.
                    for segment in definition:
                        if 'type' in segment and segment['type']:
                            used_types.add(segment['type'])
            # If the type_name is already an enum key, we still add it to the set 
            # to prevent re-inserting it in the update step.
            used_types.add(type_name)
    
    # Check blocks (Port types)
    if "blocks" in module_data:
        for block_info in module_data["blocks"].values():
            if "ports" in block_info:
                for port in block_info["ports"]:
                    if 'type' in port and port['type']:
                        used_types.add(port['type'])
                        
    return used_types

def update_bluespec_json(json_filepath, all_enum_json_data):
    """
    Loads bluespec.json, determines which modules use which enums, and inserts them.
    """
    print(f"\n💾 Updating {json_filepath} based on module usage...")
    
    try:
        with open(json_filepath, 'r') as f:
            data = json.load(f)
    except Exception as e:
        print(f"Error handling JSON file: {e}")
        sys.exit(1)
        
    # Get all keys that look like module definitions (dicts with typedefs)
    modules_to_process = [k for k in data.keys() 
                          if isinstance(data[k], dict) and 'typedefs' in data[k]]
    
    total_inserted_count = 0
    
    for module_name in modules_to_process:
        module_data = data[module_name]
        used_types = get_used_types_in_module(module_data)
        
        # Check which of the extracted enums are used in this module's types or blocks
        for enum_key, enum_definition in all_enum_json_data.items():
            if enum_key in used_types:
                # If the enum is used and not already defined, insert it
                if enum_key not in module_data["typedefs"]:
                    module_data["typedefs"][enum_key] = enum_definition
                    total_inserted_count += 1
                    print(f"   Inserted '{enum_key}' into '{module_name}'.")
    
    if total_inserted_count == 0:
         print("⚠️ No new enum definitions were inserted. All found enums were either already present or not referenced in the module usage.")
    
    # Write the modified data back to the file
    with open(json_filepath, 'w') as f:
        json.dump(data, f, indent=4)
        print(f"\n✅ Changes saved to {json_filepath}.")

def main():
    # 0. Load content
    log_content = load_log_content(LOG_FILE)
    
    # 1. Identify all enum types (Pass 1)
    confirmed_enum_types_dot = identify_enum_types(log_content)
    
    # 2. Extract members and tags, convert to JSON format (Pass 2)
    # The result is { 'test1::Colors_e': [...] }
    all_enum_json_data = extract_enum_members_and_tags(log_content, confirmed_enum_types_dot)
    
    # 3. Determine module usage and insert into bluespec.json
    update_bluespec_json(JSON_FILE, all_enum_json_data)

if __name__ == "__main__":
    main()
