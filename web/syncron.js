// Copyright © 2022-2024 David Caldwell <david@porkrind.org>
import { React, ReactDOM, jsr } from "./lib/jsml-react-bundle.js"
import { loading, card, prevent_default, human_bytes, url_with, fetch_json, fetch_text } from "./utils.js"
import { global_settings, job_settings, Saved } from "./settings.js"
import { prune_modal } from "./prune.js"

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
                view.view == "jobs" ? [jobs_view, { set_view: push_view, jobs_url: "/jobs", runs_url: "/runs" }] :
                view.view == "runs" ? [runs_view, { set_view: push_view, runs_url: view.runs_url, job: view.job }] :
                view.view == "log"  ? [log_view,  { set_view: push_view, run_url:  view.run_url,  job: view.job, run_id: view.run_id }]
                                    : ["div", { className: "alert alert-danger" }, "Can't happen"]]);
}

function nav({el, children}) {
    return ReactDOM.createPortal(children, el);
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
    Settings: ['svg', { xmlns:'http://www.w3.org/2000/svg',width:'16',height:'16',fill:'currentColor',className:'bi bi-gear',viewBox:'0 0 16 16' },
               ['path', { d:'M8 4.754a3.246 3.246 0 1 0 0 6.492 3.246 3.246 0 0 0 0-6.492M5.754 8a2.246 2.246 0 1 1 4.492 0 2.246 2.246 0 0 1-4.492 0' }],
               ['path', { d:'M9.796 1.343c-.527-1.79-3.065-1.79-3.592 0l-.094.319a.873.873 0 0 1-1.255.52l-.292-.16c-1.64-.892-3.433.902-2.54 2.541l.159.292a.873.873 0 0 1-.52 1.255l-.319.094c-1.79.527-1.79 3.065 0 3.592l.319.094a.873.873 0 0 1 .52 1.255l-.16.292c-.892 1.64.901 3.434 2.541 2.54l.292-.159a.873.873 0 0 1 1.255.52l.094.319c.527 1.79 3.065 1.79 3.592 0l.094-.319a.873.873 0 0 1 1.255-.52l.292.16c1.64.893 3.434-.902 2.54-2.541l-.159-.292a.873.873 0 0 1 .52-1.255l.319-.094c1.79-.527 1.79-3.065 0-3.592l-.319-.094a.873.873 0 0 1-.52-1.255l.16-.292c.893-1.64-.902-3.433-2.541-2.54l-.292.159a.873.873 0 0 1-1.255-.52zm-2.633.283c.246-.835 1.428-.835 1.674 0l.094.319a1.873 1.873 0 0 0 2.693 1.115l.291-.16c.764-.415 1.6.42 1.184 1.185l-.159.292a1.873 1.873 0 0 0 1.116 2.692l.318.094c.835.246.835 1.428 0 1.674l-.319.094a1.873 1.873 0 0 0-1.115 2.693l.16.291c.415.764-.42 1.6-1.185 1.184l-.291-.159a1.873 1.873 0 0 0-2.693 1.116l-.094.318c-.246.835-1.428.835-1.674 0l-.094-.319a1.873 1.873 0 0 0-2.692-1.115l-.292.16c-.764.415-1.6-.42-1.184-1.185l.159-.291A1.873 1.873 0 0 0 1.945 8.93l-.319-.094c-.835-.246-.835-1.428 0-1.674l.319-.094A1.873 1.873 0 0 0 3.06 4.377l-.16-.292c-.415-.764.42-1.6 1.185-1.184l.292.159a1.873 1.873 0 0 0 2.692-1.115z' }]],
}

function human_status(status) {
    return status == void 0     ? "..." :
           'ServerTimeout' == status ? 'Timeout: Client disappeared'                :
           'ClientTimeout' == status ? 'Timeout: Job took too long'                 :
           'Exited'        in status ? `Exited with status ${status.Exited}`        :
           'Signal'        in status ? `Killed with signal ${status.Signal}`        :
           'CoreDump'      in status ? `Dumped Core with signal ${status.CoreDump}` : "???";
}

function status_state(run) {
    return run.status == null                                                          ? 'Running' :
           run.status.Exited != void 0 && (run.status.Exited == 0 || run.log_len == 0) ? 'Success' :
                                                                                         'Failure' ;
}

function localiso(timestamp) {
    let offset_hours = new Date().getTimezoneOffset() / 60;
    return new Date(new Date(timestamp) - offset_hours * 60 * 60 * 1000).toISOString()
        .replace(/T/, ' ').replace(/Z$/, `${offset_hours > 0 ? "-" : "+"}${offset_hours < 10 ? "0" : ""}${offset_hours}:00`);
}

function elapsed(seconds, leading_zero) {
    let h = Math.floor(seconds / 60 / 60) % 60,
        m = Math.floor(seconds / 60) % 60,
        s = seconds % 60;
    let t = `:${String(s).padStart(2,"0")}`;
    if (h > 0 || m > 0 || leading_zero)
        t = String(m)+t;
    if (h > 0)
        t = `${h}:${t.padStart(5,"0")}`;
    return t;
}

function run_status(props) {
    let status = status_state(props.run);
    return jsr([React.Fragment,
                status != "Running" && ["span", status, ["br"],
                                        ["span", { className: "status-deets" }, human_status(props.run.status) ]],
                status == "Running" && props.run.progress != null && [
                    ["div", { className: "progress" },
                     ["div", { className: "progress-bar",
                               role: "progressbar", style: { width: `${props.run.progress.percent * 100}%` }, "aria-valuenow": props.run.progress.percent * 100, "aria-valuemin": 0, "aria-valuemax": 100 }]],
                    ["span", { className: "eta" }, `ETA: ${elapsed(props.run.progress.eta_seconds)}`]],
                status == "Running" && props.run.progress == null && [
                    ["div", { className: "progress" },
                     ["div", { className: "progress-bar indeterminate",
                               role: "progressbar", style: { width: "100%" }, "aria-valuenow": 100, "aria-valuemin": 0, "aria-valuemax": 100 }]],
                    ["span", { className: "eta" }, "ETA: Unknown"]]]);
}

function delay(ms) {
  return new Promise(resolve => setTimeout(resolve, ms));
}

function use_visibility(on_focus, deps) {
    React.useEffect(() => {
        console.log("use_visibility: mounting");
        let abort = new AbortController();
        let focus_abort;
        let focus = () => {
            focus_abort = new AbortController;
            on_focus(focus_abort.signal);
        };
        document.addEventListener("visibilitychange", () => {
            console.log(`visibility changed: ${document.hidden ? "hidden" : "not hidden"}`);
            if (document.hidden)
                focus_abort.abort();
            else
                focus();
        }, { signal: abort.signal });
        focus(); // Assume if we're mounted we're focused
        return () => {
            console.log("use_visibility: unmounting");
            abort.abort();
            focus_abort.abort()
        }
    }, deps);
}


function jobs_view({jobs_url, runs_url, set_view}) {
    let [jobs, set_jobs] = React.useState(null);
    let [show_settings, set_show_settings] = React.useState(false);
    let [prune_state, set_prune_state] = React.useState(undefined);

    let _sorted = jobs?.map(job => job.latest_run).sort((a,b) => b.date-a.date);
    let latest = _sorted?.[0].date;
    let running = _sorted?.filter(r => r && r.status == null) || [];

    use_visibility(async (signal) => {
        let jobs = await fetch_json(jobs_url, { signal: signal })
        if (!jobs) return; // aborted
        set_jobs(jobs);
        let es = new EventSource(url_with("/events", [ ["topic", "job"],
                                                       ["topic", "job/+/+"],
                                                       ["topic", "job/+/+/latest"]]));
        signal.addEventListener('abort', () => { console.log("Closing EventSource"); es.close() });
        es.onmessage = (message) => {
            let event = JSON.parse(message.data);
            console.log("Got event: ", event);
            let [, user, id] = event.topic.split("/");
            const update_job = (job_updater) => {
                set_jobs((old_jobs) => {
                    let new_jobs = old_jobs.concat([]);
                    let i = new_jobs.findIndex(job => job.id == id && job.user == user);
                    if (i == -1) // job_create event _should_ have happened before this so this shouldn't ever happen...
                        return new_jobs;
                    job_updater(new_jobs[i]);
                    return new_jobs;
                });
            };
            if ("job_create" in event)
                set_jobs(old_jobs => old_jobs.concat([event.job_create]));
            if ("job_update" in event)
                update_job((j) => Object.assign(j, event.job_update));
            if ("run_create" in event)
                update_job((j) => j.latest_run = event.run_create);
            if ("run_update" in event)
                update_job((j) => j.latest_run = event.run_update);
            if ("run_update_log_len" in event)
                update_job((j) => j.latest_run.log_len = event.run_update_log_len);
            if ("run_update_progress" in event)
                update_job((j) => j.latest_run.progress = event.run_update_progress);
            if ("run_delete" in event)
                update_job((j) => delete j.latest_run);
        }
    }, [set_jobs, jobs_url]);

    let prune = React.useCallback(async () => {
        set_prune_state({ pruning: true, progress: { message: "Starting…" } });
        try {
            let to_prune = (await Promise.all(jobs.map(async j => ({job: j, settings: await fetch_json(j.settings_url)}))))
                .filter(({job,settings}) => settings.retention == "default")
                .map(({job,settings}) => job);
            set_prune_state({ pruning: true, progress: { index: 0, max: to_prune.length, message: "Starting…" } });
            let result = { pruned: [], stats: { kept: { runs: 0, size: 0 }, pruned: { runs: 0, size: 0 } } };
            for (let [i, job] of to_prune.entries()) {
                set_prune_state({ pruning: true, progress: { index: i, max: to_prune.length, message: `Pruning ${job.user} / ${job.name}…`} });
                let prune_result = await fetch_json(job.prune_url, { method: 'POST' });
                result.pruned = result.pruned.concat(prune_result.pruned);
                result.stats.kept.runs   += prune_result.stats.kept.runs;
                result.stats.kept.size   += prune_result.stats.kept.size;
                result.stats.pruned.runs += prune_result.stats.pruned.runs;
                result.stats.pruned.size += prune_result.stats.pruned.size;
            }
            set_prune_state({ pruning: false, result, progress: { index: to_prune.length, max: to_prune.length, message: `Pruning Complete.`} });
        } catch(e) {
            set_prune_state(curr => Object.assign({}, curr, { pruning: false, error: e }));
        }
    }, [jobs, set_prune_state]);

    return jsr([card, { kind: "jobs-view", title: "Jobs",
                        extra_header: ['a', { href: "#", onClick: prevent_default(() => set_show_settings(true)) },
                                       svg.Settings] },
                show_settings && [global_settings, { jobs, close_settings: (reason) => { set_show_settings(false);
                                                                                         if (reason == Saved) prune() } }],
                prune_state && [prune_modal, { prune_state, done: () => set_prune_state(undefined) }],
                    jobs == null ? [loading]
                                 : [["table", { className: "jobs" },
                                     ["thead",
                                      ["tr",
                                       ["th", { scope: "col", className: "icon" } ],
                                       ["th", { scope: "col", className: "user" }, "User"],
                                       ["th", { scope: "col", className: "name" }, "Name"],
                                       ["th", { scope: "col", className: "date" }, "Last Run Date"],
                                       ["th", { scope: "col", className: "time" }, "Time"],
                                       ["th", { colspan: "2", scope: "col", className: "status" }, "Status"]]],
                                     ["tbody",
                                      jobs.sort((a,b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase())).map((job) => {
                                          if (job.latest_run == undefined)
                                              return [React.Fragment,
                                                      ["tr", { key: job.user+job.id, className: "empty" },
                                                       ["td"],
                                                       ["td", job.user],
                                                       ["td", ["a", { href: "#", onClick: prevent_default(() => set_view({ view:"runs", runs_url: job.runs_url, job:job })) }, job.name ]],
                                                       ["td", { colspan: 4 }, "No runs for this job"]],
                                                      ["tr", { key: job.user+job.id+"success-chart", className: "hist" },
                                                       ["td", { colspan: 7 }, [success_chart, { success_url: job.success_url, last_run_at: 0, last_run_status: 0 }]]]];
                                          let status = status_state(job.latest_run);
                                          return [React.Fragment,
                                                  ["tr", { key: job.user+job.id, className: status },
                                                  ["td", svg[status] ],
                                                  ["td", job.user ],
                                                  ["td", ["a", { href: "#", onClick: prevent_default(() => set_view({ view:"runs", runs_url: job.runs_url, job:job })) }, job.name ]],
                                                  ["td", localiso(job.latest_run.date) ],
                                                  ["td", { className: "time" }, elapsed(Math.floor(job.latest_run.duration_ms/1000), true)],
                                                  ["td", [run_status, {run:job.latest_run} ]],
                                                  ["td", { className: "logs-button" },
                                                   ["button", { type: "button", className: status+(job.latest_run.log_len == 0 && status != "Running" ? " disabled" : ""),
                                                                onClick: prevent_default(() => set_view({ view:"log", run_url:job.latest_run.url, job:job, run_id:job.latest_run.id})) },
                                                    status == "Running" ? "Tail Log" : "Last Log", ]]],
                                                  ["tr", { key: job.user+job.id+"success-chart", className: "hist" },
                                                   ["td", { colspan: 7 }, [success_chart, { success_url: job.success_url, last_run_at: job.latest_run.date, last_run_status: status }]]],
                                                  ];
                                      }),
                                     ]],
                                    jobs.length == 0 && [['h3', "There are no jobs."], ["a", { href: "/docs/adding-jobs" }, "How do I add jobs?"]],
                                   ]]);
}

function success_chart({success_url, last_run_at, last_run_status}) {
    let [successes, set_successes] = React.useState(null);

    React.useEffect(() => {
        let cancelled = false;
        (async () => {
            let successes = await fetch_json(url_with(success_url, {after: Date.now() - 30*24*3600*1000}))
            if (!cancelled) set_successes(successes);
        })();
        return () => cancelled = true;
    }, [success_url, last_run_at, last_run_status]);

    let canvas_ref = use_canvas((ctx, canvas) => {
        if (canvas.width != canvas.clientWidth) canvas.width = canvas.clientWidth;
        if (canvas.height != canvas.clientHeight) canvas.height = canvas.clientHeight;
        ctx.fillStyle = "black";
        ctx.fillRect(0,0,canvas.width,canvas.height);
        if (!successes) return;
        let success = getComputedStyle(window.document.body).getPropertyValue('--bs-success');
        let failure = getComputedStyle(window.document.body).getPropertyValue('--bs-danger');
        let days = 30;
        let day = Array.from(Array(days)).map(_=>[]);
        let ms__day = 24*3600*1000;
        let x__day = canvas.width / days;
        let now = Date.now();
        let start = now - days*ms__day;
        let gap_px = 2;
        // Break the runs up into days based on their start times (days ago, not calendar days).
        // Also note that when we subdivide later on we don't take time into account, we just divide the day
        // up evenly based on how many jobs are there.
        for (let h of successes)
            day[Math.max(0, Math.floor((Math.min(now-1,h[0])-start)/ms__day))].push(h);
        for (let [d, h] of day.entries()) {
            if (h.length == 0)
                h.push([]); // make the loop run and color it grey.
            let start_px = gap_px/2+Math.round(d*x__day);
            let pixels = gap_px/2+Math.round((d+1)*x__day) - start_px - gap_px;

            if (h.length <= pixels) { // This handles >= 1 run per day up to exactly 1 pixel per run
                let sub_gap = h.length <= pixels/2 ? 1 : 0; // If we can fit a border around each entry, add one.
                let x__run = (pixels + sub_gap) / h.length; // this + is counter-intuitive to me! But I worked it out on paper.
                for (let [j, r] of h.entries()) {
                    ctx.fillStyle = r[1] == undefined ? "#444" :
                                    r[1] == true      ? success :
                                    r[1] == false     ? failure : "pink";
                    let width = (start_px + Math.round((j+1)*x__run)) - (start_px + Math.round(j*x__run)) - sub_gap;
                    ctx.fillRect(start_px + Math.round(j*x__run), 0, width, canvas.height);
                }
            } else { // In this case we have N runs per px on the screen. So go through and bucket it again as above (except don't worry about start times).
                let px = Array.from(Array(days));
                let px__run = pixels / h.length;
                for (let [j, r] of h.entries()) {
                    let p = Math.floor(j*px__run);
                    // This table prioritizes failures and de-prioritizes gaps (undefined)
                    px[p] = px[p] == undefined             ? r[1]  :
                            px[p] == true && r[1] == false ? false :
                            px[p] == true                  ? true  :
                            px[p] == false                 ? px[p] : (_=>{debugger;throw "can't happen"})();
                }
                for (let [x, succ] of px.entries()) {
                    ctx.fillStyle = succ == undefined ? "#444" :
                                    succ == true      ? success :
                                    succ == false     ? failure : "pink";
                    ctx.fillRect(start_px + x, 0, 1, canvas.height);
                }
            }
        }
    }, [successes]);
    return jsr(['canvas', { ref: _=>canvas_ref.current=_ }]);
}

function use_canvas(draw, deps) {
    let canvas_ref = React.useRef(null);

    React.useEffect(() => {
        let canvas = canvas_ref.current;
        let context = canvas.getContext('2d');
        draw(context, canvas);
        return () => {}
    }, [draw].concat(deps||[]));
    return canvas_ref;
}

function runs_view({runs_url, job, set_view}) {
    let [runs, set_runs] = React.useState(null);
    let [show_settings, set_show_settings] = React.useState(false);
    let [prune_state, set_prune_state] = React.useState(undefined);

    let _sorted = runs?.concat([]).sort((a,b) => b.date-a.date);
    let latest = _sorted?.[0]?.date;
    let running = _sorted?.filter(r => r.status == null) || [];

    use_visibility(async (signal) => {
        let runs = await fetch_json(url_with(runs_url, { num: 100 }), { signal })
        if (!runs) return; // cancelled
        set_runs(runs);
        let es = new EventSource(url_with("/events", [ ["topic", `job/${job.user}/${job.id}`],
                                                       ["topic", `job/${job.user}/${job.id}/run/+`] ]));
        signal.addEventListener('abort', () => { console.log("Closing EventSource"); es.close() });
        es.onmessage = (message) => {
            let event = JSON.parse(message.data);
            console.log("Got event: ", event);
            let [,,,,run_id] = event.topic.split("/");
            const update_run = (run_updater) =>
                  // run id's are unique per job so we can uniqify them with a id hash key
                  set_runs((old_runs) => {
                      let run_obj = Object.fromEntries((old_runs||[]).map(r => [r.id, r]));
                      run_updater(run_obj);
                      return Object.values(run_obj);
                  });
            if ("job_update" in event)
                //FIXME: We should be able to update the Job name in the view, but it comes from app()s
                //complex internal state nonsense and I don't want to think about it right now
                ; //update_job((j) => Object.assign(j, event.job_update));
            if ("run_create" in event)
                update_run(r => r[run_id] = event.run_create);
            if ("run_update" in event)
                update_run(r => r[run_id] = event.run_update);
            if ("run_update_log_len" in event)
                update_run(r => r[run_id].log_len = event.run_update_log_len);
            if ("run_update_progress" in event)
                update_run(r => r[run_id].progress = event.run_update_progress);
            if ("run_delete" in event)
                update_run(r => delete r[run_id]);
        }
    }, [set_runs, job.id, job.user, runs_url]);

    let load_more = async (count) => {
        let new_runs = await fetch_json(url_with(runs_url, { before: Math.min(...runs.map(r => r.date)), num: count }))
        set_runs((old_runs) => new_runs.concat(old_runs || []));
    };

    let prune = async () => {
        set_prune_state({ pruning: true });
        let result = await fetch_json(job.prune_url, { method: 'POST' });
        set_prune_state({ pruning: false, result });
    };

    return jsr([card, { kind: "runs-view", title: `${job.user} / ${job.name}`,
                        extra_header: ['a', { href: "#", onClick: prevent_default(() => set_show_settings(true)) },
                                       svg.Settings ] },
                show_settings && [job_settings, { job, close_settings: (reason) => { set_show_settings(false);
                                                                                     if (reason == Saved) prune() } }],
                prune_state && [prune_modal, { job, prune_state, done: () => set_prune_state(undefined) }],
                runs == null     ? [loading] :
                runs.length == 0 ? ['h2', "No runs for this job" ]
                                 : [React.Fragment,
                                    ["table", { className: "jobs" },
                                    ["thead",
                                     ["tr",
                                      ["th", { scope: "col", className: "icon" } ],
                                      ["th", { scope: "col", className: "date" }, "Date"],
                                      ["th", { scope: "col", className: "size" }, "Log Size"],
                                      ["th", { scope: "col", className: "time" }, "Time"],
                                      ["th", { scope: "col", className: "status" }, "Status"]]],
                                    ["tbody",
                                     runs.sort((a,b) => b.date - a.date).map((run) => {
                                         let status = status_state(run);
                                         let show_log = () => set_view({ view:"log", run_url:run.url, job:job, run_id:run.id });
                                         return ["tr", { key: job.user+job.id+run.id, className: status },
                                                 ["td", svg[status] ],
                                                 ["td", ["a", { href: "#", onClick: prevent_default(show_log) }, run.id ]],
                                                 ["td", human_bytes(run.log_len)],
                                                 ["td", { className: "time" }, elapsed(Math.floor(run.duration_ms/1000), true)],
                                                 ["td", [run_status, {run:run} ]],
                                                ];
                                     }),
                                    ]],
                                    ["div", { className: "load-buttons" },
                                     ["button", "Load 100 more entries", { onClick: prevent_default(() => load_more(100)) }],
                                     ["button", "Load 1000 more entries", { onClick: prevent_default(() => load_more(1000)) }],
                                     ["button", "Load all the entries", { onClick: prevent_default(() => load_more()) }]],
                                   ]]);
}

function log_view({run_url, job, run_id}) {
    let [show_env, set_show_env] = React.useState(false);
    let [run, set_run] = React.useState(null);
    let [atbottom, set_atbottom] = React.useState(true);
    let status = run && status_state(run);
    const LARGE_CHUNK_SIZE = 1*1024*1024;

    const reload = async (signal) => {
            let last_len = run?.log_len;
            let new_run = await fetch_json(url_with(run_url, last_len ? { seek: last_len } : {}), { signal: signal } );
            if (!new_run.log && new_run.log_url) { // If the log is big enough the server won't populate it but _will_ give us a log_url that we can fetch from
                if (new_run.log_len < 3 * LARGE_CHUNK_SIZE)
                    new_run.log = await fetch_text(url_with(new_run.log_url, last_len ? { seek: last_len } : {}), { signal: signal });
                else { // Very large log file. Browsers get tripped up and it starts getting very slow, so only load part of the log in.
                    if (!last_len) {
                        new_run.log = [await fetch_text(url_with(new_run.log_url, { limit:  LARGE_CHUNK_SIZE }), { signal: signal }),
                                       { skip_from: LARGE_CHUNK_SIZE, skip_to: new_run.log_len - LARGE_CHUNK_SIZE },
                                       await fetch_text(url_with(new_run.log_url, { limit: -LARGE_CHUNK_SIZE }), { signal: signal })];
                    } else {
                        // don't need the beginning, just stuff to tack on to the end:
                        new_run.log   = await fetch_text(url_with(new_run.log_url, { seek: last_len }), { signal: signal });
                    }
                }
            }
            if (signal.aborted) return;
            set_atbottom(Math.abs(window.scrollMaxY - window.scrollY) < 5); // Hack. This is as close as I can come to right before react begins to render
            console.log(`scroll at load: atbottom:${atbottom}, scrollY:${window.scrollY}, scrollYMax:${window.scrollMaxY}`);
            set_run((old_run) => {
                new_run.log = [...(old_run?.log ?? []), ...(typeof new_run.log == "string" ? [new_run.log] : (new_run.log ?? []))];
                return new_run;
            });
    };

    use_visibility(async (signal) => {
        await reload(signal);
        if (signal.aborted) return;
        let es = new EventSource(url_with("/events", [ ["topic", `job/${job.user}/${job.id}/run/${run_id}`],
                                                       ["topic", `job/${job.user}/${job.id}/run/${run_id}/log`] ]));
        signal.addEventListener('abort', () => { console.log("Closing EventSource"); es.close() });
        es.onmessage = (message) => {
            let event = JSON.parse(message.data);
            console.log("Got event: ", event);
            const update_run = (run_updater) =>
                  set_run((old_run) => {
                      let new_run = Object.assign({}, old_run);
                      run_updater(new_run);
                      return new_run;
                  });
            if ("run_update" in event)
                update_run(r => Object.assign(r, event.run_update, { log: r.log }));
            if ("run_update_log_len" in event)
                update_run(r => r.log_len = event.run_update_log_len);
            if ("run_update_progress" in event)
                update_run(r => r.progress = event.run_update_progress);
            if ("run_log_append" in event) {
                set_atbottom(Math.abs(window.scrollMaxY - window.scrollY) < 5); // Hack. This is as close as I can come to right before react begins to render
                update_run(r => r.log = [...r.log, event.run_log_append.chunk]);
            }
            if ("run_delete" in event)
                update_run(r => r.deleted_reason = event.run_delete.reason);
        }
    }, [set_run, set_atbottom, job.id, job.user, run_id, run_url]);

    React.useLayoutEffect(() => {
        if (status == 'Running' && atbottom) {
            console.log(`Initiating scroll: ${status}, atbottom=${atbottom}`);
            window.scrollTo({top: window.scrollMaxY, behavior:"instant"});
        }
    });

    let ansi_to_html = (text) =>
        (text||"").split(/\x1b[[]([\d,;]+)m/).reduce((memo, s, i) => {
            if (i == 0)// || (i % 2 == 1) && s == "0")
                memo.push(['span']);
            if ((i % 2 == 1) && s == "0") {
                memo.push(['span']);
                return memo;
            }
            if (i % 2 == 1)
                memo.push(['span', { className: s.split(/[;,]/).map(c=>`ansi-${Number(c)}`).join(' ') }]);
            else
                memo[memo.length-1].push(s);
            return memo
        }, [])
    ;

    let load_more = async (whence) => {
        let skip_index = run.log.findIndex(e => e.skip_from != undefined);
        if (skip_index < 0) return; // Shouldn't be able to happen
        let skip = run.log[skip_index];
        let skipped = skip.skip_to - skip.skip_from;
        let size = Math.min(skipped, LARGE_CHUNK_SIZE);
        let chunk = await fetch_text(url_with(run.log_url, { seek: whence == 'start' ? skip.skip_from : skip.skip_to - size,
                                                             limit: size }));
        set_run((old_run) => {
            if (old_run.log[skip_index].skip_from != skip.skip_from ||
                old_run.log[skip_index].skip_to   != skip.skip_to)    // By the time we loaded the chunk the array had changed behind our back. So just ignore whatever we read.
                return old_run;
            let replace;
            if (skipped == size)
                replace = [ chunk ];
            else if (whence == 'start')
                replace = [ chunk, { skip_from: skip.skip_from + size, skip_to: skip.skip_to } ];
            else
                replace = [ { skip_from: skip.skip_from, skip_to: skip.skip_to - size }, chunk ];

            return Object.assign({}, old_run, { log: [].concat(old_run.log.slice(0,skip_index))
                                                       .concat(replace)
                                                       .concat(old_run.log.slice(skip_index+1)) });
        });
    };
    let format_log = (parts) => {
        let after_skip;
        return parts.map((part,i) => {
            if (part.skip_from != undefined) {
                after_skip = i+1;
                return ["span",
                        "\n",
                        ["button", "Load 1MB more from the start of the log", { onClick: prevent_default(() => load_more('start')) }],
                        "\n",
                        ["em", `    ... ${human_bytes(part.skip_to-part.skip_from)} skipped ...\n`],
                        ["button", "Load 1MB more from the end of the log", { onClick: prevent_default(() => load_more('end')) }],
                        ]
            } else
                return ansi_to_html(i == after_skip ? part.replace(/^.*$/m, '') : part) // Replace the partial line at the start of a chunk, it's ugly
        })
    };

    return jsr([card, { kind: "log-view",
                        title: [React.Fragment, svg[status], ` ${job.user} / ${job.name} on ${run ? localiso(run.date) : "…"}`] },
                    !run ? [loading]
                         : [["h2", "Command:"], ["code", run.cmd],
                            ["div", { className: `env ${show_env ? "show" : "hide"}` },
                             ["h2", { onClick: prevent_default(() => set_show_env(!show_env)) }, "Environment:"],
                             ["table",
                              ["tbody", run.env.map(([k,v]) => ["tr", ["td", ["code", k]], ["td", ["code", v]]])]]],
                            ["h2", "Output:"],
                            ["pre", ...format_log(run.log||[]), "\n", status == 'Running' ? ["div", { className: "dot-flashing" }] : human_status(run.status)]
                           ]]);
}
