all: web syncron

syncron: target/debug/syncron

release: target/release/syncron

target/debug/%: *.rs
	cargo build

target/release/%: *.rs
	cargo build --release

web: web/lib/jsml-react.js web/lib/jsml-react-bundle.js web/syncron.css
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

node_modules: package.json
	npm install

node_modules/@caldwell/jsml/jsml-react.mjs: node_modules
