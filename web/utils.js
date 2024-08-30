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

export function progress({min, max, current, message}) {
    min ??= 0;
    let percent = current == undefined || max == undefined || min-max == 0 ? undefined : (current - min) / (max - min) * 100;
    return jsr([React.Fragment,
                ['div', { className: 'progress-message' }, message],
                ['div', { className:'progress', role:'progressbar', 'aria-label':'message', 'aria-valuenow':percent ?? 0, 'aria-valuemin':0, 'aria-valuemax':100 },
                 percent == undefined ? ['div', { key: 0, className: 'progress-bar-indeterminate' } ]
                                      : ['div', { key: 1, className: 'progress-bar', style: { '--progress': `${percent}%` } } ],
                ]]);
}

export function classes(...class_names) {
    return { className: class_names.filter(c => typeof c == "string").join(' ') }
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

export class debounce {
    constructor(delay) {
        this.delay = delay;
    }

    hit(callback) {
        this.abort();
        this.id = setTimeout(() => { this.id = undefined; callback() }, this.delay);
    }

    abort() {
        if (this.id) this.id = clearTimeout(this.id);
    }
}

export function use_debounce(delay) {
    let [debouncer] = React.useState(() => new debounce(delay));

    React.useEffect(() => {
        return () => debouncer.abort();
    }, []);

    return debouncer;
}

// Stolen from movierip
function __find(p, o, f, v) {
    if (p.length == 1)
        return f(p[0], o, v);
    let nv = __find(p.slice(1), o[p[0]], f, v);
    return f(p[0], o, nv);
}
export function get_path(path, obj) {
    try { return __find(path.split('.'), obj, (k, o, v) => v !== undefined ? v : o[k]); }
    catch(e) { throw(`Error getting ${path} from ${JSON.stringify(obj)}: ${e}`) }
}
export function set_path_impure(path, obj, val) {
    try { return __find(path.split('.'), obj, (k, o, v) => { o[k] = v; return o }, val); }
    catch(e) { throw(`Error setting ${path} in ${JSON.stringify(obj)}: ${e}`) }
}
export function set_path(path, obj, val) {
    try { return __find(path.split('.'), obj, (k, o, v) => Object.assign({}, o, { [k]: v }), val); }
    catch(e) { throw(`Error setting ${path} in ${JSON.stringify(obj)}: ${e}`) }
}
