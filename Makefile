target=release
WASM=target/wasm32-unknown-unknown/$(target)/bluespec_translator.wasm 
debug:
	rm $(WASM) /home/vijayvithal/.local/share/surfer/translators/* || echo cleaned
	cargo build --target wasm32-unknown-unknown --$(target)
	cp $(WASM) /home/vijayvithal/.local/share/surfer/translators/
