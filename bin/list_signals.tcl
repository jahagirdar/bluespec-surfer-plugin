#! env bluetcl
###########Std Processing
 package require Bluetcl
 package require types
 lappend auto_path $env(BLUESPECDIR)/util/bluetcl-scripts/
 lappend auto_path /prj/bsvlib/bdir/
 lappend auto_path /usr/lib/x86_64-linux-gnu/graphviz/tcl/

 
 set major_version 2
 set minor_version 0
 set version "$major_version.$minor_version"
 
 # TODO: load rather than source ?
 global env
 namespace import ::Bluetcl::* 
 
 source $env(BLUESPECDIR)/tcllib/bluespec/portUtil.tcl
 
 # Load the types package (note: lowercase, not Types)
 package require types
 
 ######################################################################
 # processSwitches also sets these switches (feel free to set them yourself)
 #Bluetcl::flags set "-verilog -vdir obj -bdir obj -simdir obj -p obj:.:+"
 
 proc usage {} {
     puts "Usage: list_signals.tcl <ops> moduleName subModuleName subModuleName"
     puts ""
     puts "   moduleName  = name of top level module (for module ports moduleName)"
     puts "   subModuleName  = other modules which have a .ba file"
     puts ""
     puts "   switches from compile (see bsc help for more detail):"
     puts "      -p <path>       - path, if suppled to bsc command (i.e. -p obj:+)"
     puts ""
     puts "   i.e."
     puts "   list_signals.tcl -p bofile:+ mkTop mkBlock1 mkBlock2 ..."
     puts "       where program expects to find bofiles/Block.ba and module mkBlock in it"
     exit
 }
 
 puts "list_signals.tcl $argv"
 
 portUtil::processSwitches [list {p "+"} \
                                 {verilog} \
                                 {sim} \
                                 {include "/dev/null"} \
                                 {wrapper "/dev/null"} \
                                 \
                                 {makerename} \
                                 {rename  "/dev/null"} \
                                 {interface ""} \
                                 \
                                 {quiet} \
                                 {help} \
                                 {elab} \
                                 {debug}]
 
 
 if {$help == 1} {
     puts "list_signals.tcl version $version"
     puts "   portUtil.tcl version [portUtil::version]"
     usage
 }
 
 # need to know if we compiled for verilog or bluesim
 set vORs "-verilog"
 
 if {$verilog == 1} { set vORs "-verilog" }
 if {$sim     == 1} { set vORs "-sim" } 
 
 set portUtil::debug $debug
 
 
 Bluetcl::flags set $vORs -p $p
 
#############3
package require json
package require json::write

# Load module and ports
# Convert bitindexes to JSON array
proc bitindexes_to_json {typename} {
    set entries [types::createBitIndexes $typename]
    set jsonList {}
    foreach e $entries {
        foreach {rawname type width max min} $e {}
        set name [string trimleft $rawname "."]

        set obj [json::write::object \
            var [json::write::string $name] \
            type [json::write::string $type] \
            width $width \
            min $min \
            max $max \
        ]
        lappend jsonList $obj
    }
    return [json::write::array {*}$jsonList]
}

# Main function to generate JSON with blocks
proc blocks_to_json {blocks  module} {
    set jsonList {}
    set allTypes {}

    # Collect all port types
    foreach block $blocks {
        foreach {var type portspec} $block {}
        set ports [lindex $portspec 1]
        foreach p $ports {
            lappend allTypes [lindex $p 1]
        }
    }
    set allTypes [lsort -unique $allTypes]

    # Precompute all typedefs at top-level
    array set typedefsJSON {}
    foreach t $allTypes {
        if {[catch {types::createBitIndexes $t}]} { continue }
        set typedefsJSON($t) [bitindexes_to_json $t]
    }

    # Build block list (without typedefs)
    foreach block $blocks {
        foreach {var type portspec} $block {}
        set ports [lindex $portspec 1]

        set portJsonList {}
        foreach p $ports {
            set pname [lindex $p 0]
            set ptype [lindex $p 1]
            lappend portJsonList [json::write::object \
                var [json::write::string $pname] \
                type [json::write::string $ptype] \
            ]
        }

        set blockJson [json::write::object \
            type [json::write::string $type] \
            ports [json::write::array {*}$portJsonList] \
        ]

        dict set jsonList $var $blockJson
    }

    # Convert typedefs array to top-level JSON object
    set typedefsJson [json::write::object {*}[array get typedefsJSON]]

    # Return final JSON with typedefs and blocks
    set jsonoutput  [json::write::object \
        typedefs $typedefsJson \
        blocks [json::write::object {*}$jsonList] \
    ] 
# puts $jsonoutput
    return $jsonoutput
}
#set module [lindex $argv 0]
set jsonoutput {}
dict set jsonoutput top [ json::write::string [lindex $argv 0]]
foreach module $argv {
	puts $module
Bluetcl::module load $module
set ports [Bluetcl::submodule porttypes $module]
# Generate JSON and write to file
#dict set jsonoutput $module [json::write::object $module [blocks_to_json $ports  $module]]
dict set jsonoutput $module [blocks_to_json $ports  $module]
}
set filename "bluespec.json"
set fh [open $filename "w"]
puts $fh [json::write::object {*}$jsonoutput]
close $fh

