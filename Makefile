default:
	cargo build --target wasm32-unknown-unknown
	cp target/wasm32-unknown-unknown/debug/bluespec_translator.wasm /home/vijayvithal/.local/share/surfer/translators/
