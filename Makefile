default:
	cargo build --release --target wasm32-unknown-unknown
	cp target/wasm32-unknown-unknown/release/bluespec_translator.wasm /home/vijayvithal/.local/share/surfer/translators/
