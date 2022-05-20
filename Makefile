all: static syncron

syncron: target/debug/syncron

release: target/release/syncron

target/debug/%: *.rs
	cargo build

target/release/%: *.rs
	cargo build --release

static: static/lib/jsml-react.js static/lib/jsml-react-bundle.js static/syncron.css
.PHONY: static

%.css: %.scss
	./node_modules/.bin/sass $< > $@

static/lib:
	mkdir -p static/lib

static/lib/jsml-react.js: node_modules/@caldwell/jsml/jsml-react.mjs static/lib
	@test -e $@ || ln -s ../../$< $@

static/lib/jsml-react-bundle.js: node_modules package.json static/lib Makefile
	echo "let require;" > $@.temp
	./node_modules/.bin/browserify -t babelify  -r './static/lib/jsml-react.js:jsml-react' -r 'react/cjs/react.production.min.js:react' -r 'react-dom/cjs/react-dom.production.min.js:react-dom' >> $@.temp
	echo "export let React = require('react'), ReactDOM = require('react-dom'), jsr = require('jsml-react').jsr;" >> $@.temp
	mv $@.temp $@

node_modules: package.json
	npm install

node_modules/@caldwell/jsml/jsml-react.mjs: node_modules
