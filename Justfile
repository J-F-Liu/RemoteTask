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
    -rm bundle/release-system.zip
    7z a -tzip bundle/release-system.zip -r public/* .env
    cd migration && 7z a -tzip ../bundle/release-system.zip tasks.db
    cd target/release && 7z a -tzip ../../bundle/release-system.zip *.exe