.PHONY: bump

bump:
ifndef NEW
	$(error Usage: make bump NEW=0.9.0)
endif
	bin/flow bump-version $(NEW)