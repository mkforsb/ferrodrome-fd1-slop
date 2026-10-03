# Convenience targets. Requires the Dioxus CLI (`dx`, 0.7.x) and the
# wasm32-unknown-unknown target (`rustup target add wasm32-unknown-unknown`).

APP := crates/app

.PHONY: web desktop serve-web run-desktop test lint renders probe bench clean

## Release bundles: target/dx/ferrodrome/release/{web/public,linux/app}
web:
	cd $(APP) && dx build --release --web

desktop:
	cd $(APP) && dx build --release --desktop

## Serve the web build on http://127.0.0.1:8080
serve-web:
	cd $(APP) && dx serve --release --web

## Run the native Linux app (PulseAudio / PipeWire-pulse)
run-desktop:
	cd $(APP) && dx serve --release --desktop

test:
	cargo test --release -p ferrodrome-dsp -p ferrodrome-worklet -p ferrodrome

lint:
	cargo fmt --all --check
	cargo clippy --all-targets -p ferrodrome-dsp -p ferrodrome-worklet -- -D warnings
	cargo clippy --all-targets -p ferrodrome --features desktop -- -D warnings
	cargo clippy -p ferrodrome --features web --target wasm32-unknown-unknown -- -D warnings

## Every machine solo plus a few random halls, as WAV files in ./renders
renders:
	cargo run --release -p ferrodrome-dsp --example render -- renders

## Loudness of every machine (default, hits and random patches)
probe:
	cargo run --release -p ferrodrome-dsp --example probe

## Worst-case CPU: eight machines with every effect running
bench:
	cargo run --release -p ferrodrome-dsp --example bench

clean:
	cargo clean
