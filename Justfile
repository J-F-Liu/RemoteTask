set shell := ["sh", "-c"]
date := `nu -c "date now | format date %Y%m%d"`
month := `nu -c "date now | format date %Y-%m"`

show:
	@echo "Build Date: {{date}} {{month}}"

build args='':
	@echo "Building..."
	cargo build {{args}}
	@echo "Done"