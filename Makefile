.DEFAULT_GOAL := xmd

PROFILE ?= release
TARGET ?=
CARGO ?= cargo
NPM ?= npm
# CI can reuse the WASM package downloaded from an earlier job.
WASM_PREBUILT ?= 0

.PHONY: help all clients ide xmd installer zed vscode neovim helix wasm web web-site cloud

help:
	@printf '%s\n' 'make [xmd]   Build the CLI and language server (target/release/xmd)' 'make installer Build target/release/xmd-installer' 'make clients Build every editor client and the static web app' 'make all     Build xmd, the installer, all clients, and the cloud backend' 'make zed     Build extension.wasm and dist/xmd-zed.tar.gz' 'make vscode  Package dist/xmd.vsix' 'make neovim  Package dist/xmd-neovim.tar.gz' 'make helix   Package dist/xmd-helix.tar.gz' 'make wasm    Build clients/web/pkg' 'make web     Build WASM and the static site in clients/web/dist' 'make cloud   Build the web app with its cloud backend module' 'Options: PROFILE=dev, TARGET=<Rust target>, WASM_PREBUILT=1'

all: xmd installer clients cloud
clients: ide web
ide: zed vscode neovim helix

xmd:
	$(CARGO) build --locked -p xmd --profile $(PROFILE) $(if $(TARGET),--target $(TARGET))

installer:
	$(CARGO) build --locked -p xmd-installer --profile $(PROFILE) $(if $(TARGET),--target $(TARGET))

zed: | dist
	rustup target add wasm32-wasip2
	$(CARGO) build --locked --manifest-path clients/ide/zed/Cargo.toml --release --target wasm32-wasip2 --target-dir clients/ide/zed/target
	cp clients/ide/zed/target/wasm32-wasip2/release/xmd_zed_extension.wasm clients/ide/zed/extension.wasm
	tar -C clients/ide/zed -czf dist/xmd-zed.tar.gz extension.toml extension.wasm languages

vscode: clients/ide/vscode/node_modules/.package-lock.json | dist
	$(NPM) --prefix clients/ide/vscode run check
	cd clients/ide/vscode && $(NPM) exec --no -- vsce package --allow-missing-repository --skip-license --out ../../../dist/xmd.vsix

neovim: | dist
	tar -C clients/ide/neovim -czf dist/xmd-neovim.tar.gz xmd.lua

helix: | dist
	tar -C clients/ide/helix -czf dist/xmd-helix.tar.gz languages.toml runtime

wasm:
	wasm-pack build hosts/wasm --target web --out-dir ../../clients/web/pkg --out-name xmd --release --locked

web: web-site

# Keep the dependency on WASM in the graph so parallel builds cannot race it.
ifneq ($(WASM_PREBUILT),1)
web-site: wasm
endif
web-site: clients/web/node_modules/.package-lock.json
	node clients/web/scripts/build.mjs

cloud: web clients/web/cloud/node_modules/.package-lock.json
	node clients/web/cloud/build.mjs

clients/web/node_modules/.package-lock.json: clients/web/package.json clients/web/package-lock.json clients/web/apps/docs/package.json
	$(NPM) --prefix clients/web ci
	@touch $@

clients/ide/vscode/node_modules/.package-lock.json: clients/ide/vscode/package.json clients/ide/vscode/package-lock.json
	$(NPM) --prefix clients/ide/vscode ci
	@touch $@

clients/web/cloud/node_modules/.package-lock.json: clients/web/cloud/package.json clients/web/cloud/package-lock.json
	$(NPM) --prefix clients/web/cloud ci
	@touch $@

dist:
	mkdir -p $@
