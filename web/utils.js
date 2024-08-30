// Copyright © 2022-2024 David Caldwell <david@porkrind.org>
import { React, jsr } from "./lib/jsml-react-bundle.js"

export function loading({message}) {
    message ??= ["span",  { className: "visually-hidden" }, "Loading..."];
    return jsr(["div",
                ["div", { className: "spinner-border text-primary", role: "status" }],
                ["span", message]]);
}

export function card({kind, title, extra_header, children}) {
    return jsr(["div", { className: `card ${kind}` },
                ["div", { className: "card-header" },
                 ["h1", title],
                extra_header],
                ["div", { className: "card-body" },
                 children]]);
}

export function prevent_default(f) {
    return (e) => {
        e.preventDefault();
        return f();
    }
}

export function human_bytes(bytes) {
    if (bytes == 0) return bytes.toString()+"B";
    let exp = Math.floor(Math.log(bytes)/Math.log(1024));
    let s = bytes / (1024**exp);
    return s.toString().replace(/([\d.]{4}).*/, '$1') + ["B","KB","MB","GB","TB","PB","EB"][exp];
}

export function url_with(url, params) {
    let u = new URL(url, window.location.href);
    u.search = (new URLSearchParams([...u.searchParams.entries()].concat(params instanceof Array ? params : Object.entries(params)))).toString();
    return u;
}

export async function _fetch(url, options={}) {
    let headers = {};
    if (options.method == "POST" | options.method == "PUT")
        headers = { headers: { "Content-Type": "application/json", ...options.headers ?? {} } };
    try {
        let resp = await window.fetch(url, {...options, ...headers });
        if (!resp.ok) throw("Response failed: "+resp.statusText)
        return resp;
    } catch(e) {
        if (e.code == DOMException.ABORT_ERR) return undefined;
        throw e;
    }
}

export async function fetch_json(url, options={}) {
    let resp = await _fetch(url, options);
    return resp?.json()
}

export async function fetch_text(url, options={}) {
    let resp = await _fetch(url, options)
    return resp?.text()
}
