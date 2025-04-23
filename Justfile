set shell := ["sh", "-c"]

webpage:
	cd webpage && dx bundle --platform web
	-rm public/index.html
	-rm public/assets/*
	cp -r webpage/target/dx/webpage/release/web/public/* public

run args='':
	just webpage
	cargo run {{args}}

build args='':
	@echo "Building..."
	just webpage
	cargo build {{args}}
	@echo "Done"

bundle:
    just build --release
    -rm bundle/remote-task.zip
    7z a -tzip bundle/remote-task.zip -r public/* .env
    cd target/release && 7z a -tzip ../../bundle/remote-task.zip *.exe