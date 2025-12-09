WASM=target/wasm32-unknown-unknown/debug/bluespec_translator.wasm 
default:
	rm $(WASM) /home/vijayvithal/.local/share/surfer/translators/* || echo cleaned
	cargo build --target wasm32-unknown-unknown
	cp $(WASM) /home/vijayvithal/.local/share/surfer/translators/
