default:
	cargo build --release
	cp ../target/wasm32-unknown-unknown/release/Bluespec_Translator.wasm /home/vijayvithal/.local/share/surfer/translators/
