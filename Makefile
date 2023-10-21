all: web syncron

syncron: target/debug/syncron

release: target/release/syncron

target/debug/%: *.rs payload.zip
	cargo build
	cat $@ payload.zip > $@.new
	chmod +x $@.new
	mv $@.new $@

target/release/%: *.rs payload.zip
	cargo build --release
	cat $@ payload.zip > $@.new
	chmod +x $@.new
	mv $@.new $@

WEB_TARGETS=web/lib/jsml-react.js web/lib/jsml-react-bundle.js web/syncron.css
web: $(WEB_TARGETS)
.PHONY: web

%.css: %.scss
	./node_modules/.bin/sass $< > $@

web/lib:
	mkdir -p web/lib

web/lib/jsml-react.js: node_modules/@caldwell/jsml/jsml-react.mjs web/lib
	@test -e $@ || ln -s ../../$< $@

web/lib/jsml-react-bundle.js: node_modules package.json web/lib Makefile
	echo "let require;" > $@.temp
	./node_modules/.bin/browserify -t babelify  -r './web/lib/jsml-react.js:jsml-react' -r 'react/cjs/react.production.min.js:react' -r 'react-dom/cjs/react-dom.production.min.js:react-dom' >> $@.temp
	echo "export let React = require('react'), ReactDOM = require('react-dom'), jsr = require('jsml-react').jsr;" >> $@.temp
	mv $@.temp $@

payload.zip: web docs
	zip -ru $@ $$(git ls-files docs web | grep -v scss) $(WEB_TARGETS) || test $$? = 12

node_modules: package.json
	npm install

node_modules/@caldwell/jsml/jsml-react.mjs: node_modules
