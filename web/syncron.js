// Copyright © 2022 David Caldwell <david@porkrind.org>
import { React, ReactDOM, jsr } from "./lib/jsml-react-bundle.js"

function main() {
    let [nav_el, app_view] = ["nav", "app-view"].map(id => document.getElementById(id));

    ReactDOM.render(jsr([app, Object.assign({ nav_el: nav_el }, history.state ? { initial_view:history.state } : {})]), app_view);
}
window.onload = main;

function app({nav_el, initial_view}) {
    let [view, set_view] = React.useState(initial_view || {view: "jobs"});

    const push_view = (view) => {
        history.pushState(view, "", view.view == "jobs" ? '#' :
                                    view.view == "runs" ? `#${view.job.user}/${view.job.name}` :
                                    view.view == "log"  ? `#${view.job.user}/${view.job.name}/${view.run_id}` : '#cant-happen');
        set_view(view);
    }

    React.useEffect(() => {
        let old_onpopstate = window.onpopstate;
        window.onpopstate = (event) => {
            set_view(event.state || {view: "jobs"});
        };

        () => window.onpopstate = old_onpopstate
    });

    let crumbs = view.view == "jobs" ? [{ id:"Jobs" }] :
                 view.view == "runs" ? [{ id:"Jobs",        click:() => push_view({ view:"jobs" }) },
                                        { id:view.job.user, click:() => {} },
                                        { id:view.job.name }] :
                 view.view == "log"  ? [{ id:"Jobs",        click:() => push_view({ view:"jobs" }) },
                                        { id:view.job.user, click:() =>{} },
                                        { id:view.job.name, click:() => push_view({ view:"runs", runs_url:view.job.runs_url, job:view.job }) },
                                        { id:view.run_id }]
                                     : [{ id: "can't happen" }];
    return jsr([React.Fragment,
                [nav, { el: nav_el },
                 ["nav", { "aria-label": "breadcrumb" },
                  ["ol", { className: "breadcrumb" },
                   crumbs.map((crumb) =>
                       crumb.click ? ["li", { className: "breadcrumb-item" },        ["a", { href:"#", onClick:prevent_default(crumb.click) }, crumb.id]]
                                   : ["li", { className: "breadcrumb-item active" },                                                           crumb.id]),
                  ]]],
                view.view == "jobs" ? [jobs_view, { set_view: push_view, jobs_url: "/jobs" }] :
                view.view == "runs" ? [runs_view, { set_view: push_view, runs_url: view.runs_url, job: view.job }] :
                view.view == "log"  ? [log_view,  { set_view: push_view, run_url:  view.run_url,  job: view.job }]
                                    : ["div", { className: "alert alert-danger" }, "Can't happen"]]);
}

function nav({el, children}) {
    return ReactDOM.createPortal(children, el);
}

async function fetch_json(url, options={}) {
    let resp = await fetch(url, options)
    if (!resp.ok) throw("Response failed: "+resp.statusText)
    return resp.json()
}

const svg = {
    Success: ["svg", { xmlns: "http://www.w3.org/2000/svg", width: "32", height: "32", fill: "currentColor", className: "bi bi-check-circle-fill text-success", viewBox: "0 0 16 16" },
              ["path", { d: "M16 8A8 8 0 1 1 0 8a8 8 0 0 1 16 0zm-3.97-3.03a.75.75 0 0 0-1.08.022L7.477 9.417 5.384 7.323a.75.75 0 0 0-1.06 1.06L6.97 11.03a.75.75 0 0 0 1.079-.02l3.992-4.99a.75.75 0 0 0-.01-1.05z" }]],
    Failure: ["svg", { xmlns: "http://www.w3.org/2000/svg", width: "32", height: "32", fill: "currentColor", className: "bi bi-exclamation-triangle-fill text-danger", viewBox: "0 0 16 16" },
              ["path", { d: "M8.982 1.566a1.13 1.13 0 0 0-1.96 0L.165 13.233c-.457.778.091 1.767.98 1.767h13.713c.889 0 1.438-.99.98-1.767L8.982 1.566zM8 5c.535 0 .954.462.9.995l-.35 3.507a.552.552 0 0 1-1.1 0L7.1 5.995A.905.905 0 0 1 8 5zm.002 6a1 1 0 1 1 0 2 1 1 0 0 1 0-2z" }]],
    Running: ["svg", { xmlns: "http://www.w3.org/2000/svg", width: "32", height: "32", fill: "currentColor", className: "bi bi-hypnotize text-info", viewBox: "0 0 16 16" },
              ["path", { d: "m7.949 7.998.006-.003.003.009-.01-.006Zm.025-.028v-.03l.018.01-.018.02Zm0 .015.04-.022.01.006v.04l-.029.016-.021-.012v-.028Zm.049.057v-.014l-.008.01.008.004Zm-.05-.008h.006l-.006.004v-.004Z" }],
              ["path", { fillRule: "evenodd", d: "M8 0a8 8 0 1 0 0 16A8 8 0 0 0 8 0ZM4.965 1.69a6.972 6.972 0 0 1 3.861-.642c.722.767 1.177 1.887 1.177 3.135 0 1.656-.802 3.088-1.965 3.766 1.263.24 2.655-.815 3.406-2.742.38-.975.537-2.023.492-2.996a7.027 7.027 0 0 1 2.488 3.003c-.303 1.01-1.046 1.966-2.128 2.59-1.44.832-3.09.85-4.26.173l.008.021.012-.006-.01.01c.42 1.218 2.032 1.9 4.08 1.586a7.415 7.415 0 0 0 2.856-1.081 6.963 6.963 0 0 1-1.358 3.662c-1.03.248-2.235.084-3.322-.544-1.433-.827-2.272-2.236-2.279-3.58l-.012-.003c-.845.972-.63 2.71.666 4.327a7.415 7.415 0 0 0 2.37 1.935 6.972 6.972 0 0 1-3.86.65c-.727-.767-1.186-1.892-1.186-3.146 0-1.658.804-3.091 1.969-3.768l-.002-.007c-1.266-.25-2.666.805-3.42 2.74a7.415 7.415 0 0 0-.49 3.012 7.026 7.026 0 0 1-2.49-3.018C1.87 9.757 2.613 8.8 3.696 8.174c1.438-.83 3.084-.85 4.253-.176l.005-.006C7.538 6.77 5.924 6.085 3.872 6.4c-1.04.16-2.03.55-2.853 1.08a6.962 6.962 0 0 1 1.372-3.667l-.002.003c1.025-.243 2.224-.078 3.306.547 1.43.826 2.269 2.23 2.28 3.573L8 7.941c.837-.974.62-2.706-.673-4.319a7.415 7.415 0 0 0-2.362-1.931Z" }]],
    Refresh: ["svg", { xmlns:"http://www.w3.org/2000/svg", width: "32", height: "32", fill: "currentColor", className: "bi bi-arrow-repeat", viewBox: "0 0 16 16" },
              ["path", { d: "M11.534 7h3.932a.25.25 0 0 1 .192.41l-1.966 2.36a.25.25 0 0 1-.384 0l-1.966-2.36a.25.25 0 0 1 .192-.41zm-11 2h3.932a.25.25 0 0 0 .192-.41L2.692 6.23a.25.25 0 0 0-.384 0L.342 8.59A.25.25 0 0 0 .534 9z" }],
              ["path", { fillRule: "evenodd", d: "M8 3c-1.552 0-2.94.707-3.857 1.818a.5.5 0 1 1-.771-.636A6.002 6.002 0 0 1 13.917 7H12.9A5.002 5.002 0 0 0 8 3zM3.1 9a5.002 5.002 0 0 0 8.757 2.182.5.5 0 1 1 .771.636A6.002 6.002 0 0 1 2.083 9H3.1z" }]],
}

function human_status(status) {
    return status == void 0     ? "..." :
           'Exited'   in status ? `Exited with status ${status.Exited}`        :
           'Signal'   in status ? `Killed with signal ${status.Signal}`        :
           'CoreDump' in status ? `Dumped Core with signal ${status.CoreDump}` : "???";
}

function status_state(run) {
    return run.status == null                               ? 'Running' :
           (run.status.Exited??-1) == 0 || run.log_len == 0 ? 'Success' :
                                                              'Failure' ;
}

function localiso(timestamp) {
    let offset_hours = new Date().getTimezoneOffset() / 60;
    return new Date(new Date(timestamp*1000) - offset_hours * 60 * 60 * 1000).toISOString()
        .replace(/T/, ' ').replace(/Z$/, `${offset_hours > 0 ? "-" : "+"}${offset_hours < 10 ? "0" : ""}${offset_hours}:00`);
}

function prevent_default(f) {
    return (e) => {
        e.preventDefault();
        return f();
    }
}

function human_bytes(bytes) {
    if (bytes == 0) return bytes.toString()+"B";
    let exp = Math.floor(Math.log(bytes)/Math.log(1024));
    let s = bytes / (1024**exp);
    return s.toString().replace(/([\d.]{4}).*/, '$1') + ["B","KB","MB","GB","TB","PB","EB"][exp];
}

function run_status(props) {
    let status = status_state(props.run);
    return jsr([React.Fragment,
                status != "Running" && ["span", status, ["br"],
                                        ["span", { className: "status-deets" }, human_status(props.run.status) ]],
                status == "Running" && props.run.progress != null && [
                    ["div", { className: "progress" },
                     ["div", { className: "progress-bar",
                               role: "progressbar", style: { width: props.run.progress.percent * 100 }, "aria-valuenow": props.run.progress.percent * 100, "aria-valuemin": 0, "aria-valuemax": 100 }]],
                    ["span", { className: "eta" }, `ETA: ${props.run.progress.eta_seconds}`]],
                status == "Running" && props.run.progress == null && [
                    ["div", { className: "progress" },
                     ["div", { className: "progress-bar indeterminate",
                               role: "progressbar", style: { width: "100%" }, "aria-valuenow": 100, "aria-valuemin": 0, "aria-valuemax": 100 }]],
                    ["span", { className: "eta" }, "ETA: Unknown"]]]);
}

function loading({}) {
    return jsr(["div", { className: "spinner-border text-primary", role: "status" },
                ["span",  { className: "visually-hidden" }, "Loading..." ]]);
}

function card(kind, title, body) {
    return ["div", { className: `card ${kind}` },
            ["div", { className: "card-header" },
             ["h1", title]],
            ["div", { className: "card-body" },
             body]];
}

function synced_interval(period, offset, callback) {
    let id = setTimeout(() => { // Sync up to *:02 and then interval over a minute
        callback();
        id = setInterval(callback, period);
    }, 60*1000 - Date.now() % period + offset);
    return () => clearInterval(id);
}

function jobs_view({jobs_url, set_view}) {
    let [jobs, set_jobs] = React.useState(null);
    React.useEffect(() => {
        async function reload() {
            set_jobs(await fetch_json(jobs_url));
        }
        reload();
        return synced_interval(60*1000, 2000, reload);
    }, [jobs_url]);
    return jsr(card("jobs-view",
                    "Jobs",
                    jobs == null ? [loading]
                                 : ["table", { className: "jobs" },
                                    ["thead",
                                     ["tr",
                                      ["th", { scope: "col", className: "icon" } ],
                                      ["th", { scope: "col", className: "user" }, "User"],
                                      ["th", { scope: "col", className: "name" }, "Name"],
                                      ["th", { scope: "col", className: "name" }, "Last Run Date"],
                                      ["th", { colspan: "2", scope: "col", className: "status" }, "Status"]]],
                                    ["tbody",
                                     jobs.map((job) => {
                                         let status = status_state(job.latest_run);
                                         return ["tr", { key: job.user+job.id, className: status },
                                                 ["td", svg[status] ],
                                                 ["td", job.user ],
                                                 ["td", ["a", { href: "#", onClick: prevent_default(() => set_view({ view:"runs", runs_url: job.runs_url, job:job })) }, job.name ]],
                                                 ["td", localiso(job.latest_run.date) ],
                                                 ["td", [run_status, {run:job.latest_run} ]],
                                                 ["td", { className: "logs-button" },
                                                  ["button", { type: "button", className: status+(job.latest_run.log_len == 0 && status != "Running" ? " disabled" : ""),
                                                               onClick: prevent_default(() => set_view({ view:"log", run_url:job.latest_run.url, job:job, run_id:job.latest_run.id})) },
                                                   status == "Running" ? "Tail Log" : "Last Log", ]]];
                                     }),
                                    ]]));
}

function runs_view({runs_url, job, set_view}) {
    let [runs, set_runs] = React.useState(null);
    React.useEffect(() => {
        async function reload() {
            set_runs(await fetch_json(runs_url));
        }
        reload();
        return synced_interval(60*1000, 2000, reload);
    }, [runs_url]);
    return jsr(card("runs-view",
                    `${job.user} / ${job.name}`,
                    runs == null ? [loading]
                                 : ["table", { className: "jobs" },
                                    ["thead",
                                     ["tr",
                                      ["th", { scope: "col", className: "icon" } ],
                                      ["th", { scope: "col", className: "date" }, "Date"],
                                      ["th", { scope: "col", className: "size" }, "Log Size"],
                                      ["th", { scope: "col", className: "status" }, "Status"]]],
                                    ["tbody",
                                     runs.sort((a,b) => b.date - a.date).map((run) => {
                                         let status = status_state(run);
                                         let show_log = () => set_view({ view:"log", run_url:run.url, job:job, run_id:run.id });
                                         return ["tr", { key: job.user+job.id+run.id, className: status },
                                                 ["td", svg[status] ],
                                                 ["td", ["a", { href: "#", onClick: prevent_default(show_log) }, run.id ]],
                                                 ["td", human_bytes(run.log_len)],
                                                 ["td", [run_status, {run:run} ]],
                                                ];
                                     }),
                                    ]]));
}

function log_view({run_url, job}) {
    let [show_env, set_show_env] = React.useState(false);
    let [run, set_run] = React.useState(null);
    let [atbottom, set_atbottom] = React.useState(true);
    let status = run && status_state(run);

    React.useEffect(() => {
        async function reload() {
            let new_run = await fetch_json(run_url);
            set_atbottom(Math.abs(window.scrollMaxY - window.scrollY) < 5); // Hack. This is as close as I can come to right before react begins to render
            console.log(`scroll at load: atbottom:${atbottom}, scrollY:${window.scrollY}, scrollYMax:${window.scrollMaxY}`);
            set_run(new_run);
            if (new_run.status != null) // Stop refreshing once the run is finished
                clearInterval(id);
        }

        reload();
        let id = setInterval(reload, 1*1000);
        return () => clearInterval(id);
    }, [run_url]);

    React.useLayoutEffect(() => {
        if (status == 'Running' && atbottom) {
            console.log(`Initiating scroll: ${status}, atbottom=${atbottom}`);
            window.scrollTo({top: window.scrollMaxY, behavior:"instant"});
        }
    });

    return jsr(card("log-view",
                    [React.Fragment, svg[status], ` ${job.user} / ${job.name} on ${run ? localiso(run.date) : "…"}`],
                    !run ? [loading]
                         : [["h2", "Command:"], ["code", run.cmd],
                            ["div", { className: `env ${show_env ? "show" : "hide"}` },
                             ["h2", { onClick: prevent_default(() => set_show_env(!show_env)) }, "Environment:"],
                             ["table",
                              ["tbody", run.env.map(([k,v]) => ["tr", ["td", ["code", k]], ["td", ["code", v]]])]]],
                            ["h2", "Output:"],
                            ["pre", run.log, "\n", status == 'Running' ? ["div", { className: "dot-flashing" }] : human_status(run.status)]
                           ]));
}
