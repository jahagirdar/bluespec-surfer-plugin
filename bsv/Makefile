SHELL = /bin/zsh
default:
	bsc -elab -verilog -dsyminitial test1.bsv >err
	bsc -sim -o bsim -e mkTop 
	list_signals.tcl mkTop mkA
	python3 ../bin/update_enum.py
