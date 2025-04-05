set shell := ["sh", "-c"]
date := `nu -c "date now | format date %Y%m%d"`
month := `nu -c "date now | format date %Y-%m"`

show:
	@echo "Build Date: {{date}} {{month}}"

build:
	@echo "Building..."
	cargo build --release
	@echo "Done"