// Copyright © 2024 David Caldwell <david@porkrind.org>
import { React, jsr } from "./lib/jsml-react-bundle.js"
import { loading, card, classes, prevent_default, human_bytes, url_with, fetch_json, _fetch, use_debounce, set_path } from "./utils.js"

const Saving = Symbol("Saving");
const ValidationError = Symbol("Validation Error");
export const Saved = Symbol("Saved");
export const Canceled = Symbol("Canceled");
function settings_modal({kind, title, /*validate,*/ save_settings, close_settings, children}) {
    let [save_state, set_save_state] = React.useState();

    const save = async () => {
        set_save_state(Saving);
        try {
            let resp = await save_settings();
            set_save_state(undefined);
            if (!resp.ok) {
                set_save_state(`Couldn't save: ${resp.statusText} [${resp.statusCode}]`);
                return;
            }
        } catch(e) {
            set_save_state(undefined);
            if (e != ValidationError)
                throw(e)
            return;
        }
        close_settings(Saved);
    };

    return jsr(['div',
                ['div', { className: "backdrop" }],
                [card, { kind: `modal ${kind}`, title },
                 children,
                 ['div', { className: "buttons" },
                  ['button', { disabled: save_state == Saving, onClick: prevent_default(() => save() ) }, save_state == Saving ? [loading, { message: "Saving..." }] : 'Save'],
                  ['button', { disabled: save_state == Saving, onClick: prevent_default(() => close_settings(Canceled) ) }, 'Cancel']],
                 typeof save_state == "string" && ['div', { className: "error" }, save_state],
                ]]);
}

function use_retention_state() {
    let [retention, set_retention] = React.useState(null);
    const size_units = {tb:1024**4, gb:1024**3, mb:1024**2, kb:1024**1, b:1024**0};
    const time_units = {y:365, m:30, w:7, d:1};
    const into_retention_state = (retention_api) => {
            set_retention({
                default: retention_api == "default",
                time: (enable => {
                    let [units, days] = Object.entries(time_units).find(([k,v]) => (retention_api.max_age ?? 0) % v == 0);
                    return {
                        enable: enable,
                        value: !enable ? "" : retention_api.max_age / days,
                        units: !enable ? "d" : units,
                        error: undefined,
                    }
                })(retention_api.max_age != undefined),
                runs: {
                    enable: retention_api.max_runs != undefined,
                    value: retention_api.max_runs ?? "",
                    error: undefined,
                },
                size: (enable => {
                    let [units, bytes] = Object.entries(size_units).find(([k,v]) => (retention_api.max_size ?? 0) % v == 0);
                    return {
                        enable: enable,
                        value: !enable ? ""   : retention_api.max_size / bytes,
                        units: !enable ? "mb" : units,
                        error: undefined,
                    };
                })(retention_api.max_size != undefined),
            });
    };
    const from_retention_state = () => {
        const validate = (kind, description) => {
            let bad = retention[kind].enable && retention[kind].value === "";
            set_retention_path(`${kind}.error`, bad ? `Please provide a ${description}` : undefined);
            return bad;
        };
        if (validate("time", "maximum age")       | // `|` instead of `||` so no short-cicuiting happens
            validate("runs", "maximum run count") |
            validate("size", "maximum size"))
            throw ValidationError;
        return retention.default ? "default" : {
            max_age:  retention.time.enable ? retention.time.value * time_units[retention.time.units] : undefined,
            max_runs: retention.runs.enable ? retention.runs.value * 1                                : undefined,
            max_size: retention.size.enable ? retention.size.value * size_units[retention.size.units] : undefined,
        }
    }
    const set_retention_path = (setting_path, val) => set_retention(prev => set_path(setting_path, prev, val));
    return [retention, into_retention_state, from_retention_state, set_retention_path];
}

export function global_settings({jobs, close_settings}) {
    let [retention, into_retention_state, from_retention_state, set_retention_path] = use_retention_state();
    let [save_state, set_save_state] = React.useState();

    React.useEffect(() => {
        let cancelled = false;
        (async () => {
            let settings = (await fetch_json("/settings"));
            if (cancelled) return;
            into_retention_state(settings.retention);
        })();
        return () => cancelled = true;
    }, ["/settings"]);

    const save_settings = async () => await _fetch("/settings", {
        method: "PUT",
        body: JSON.stringify({ retention: from_retention_state() })
    });

    const prune_dry_run = React.useCallback(async () => {
        let total_results = { pruned: [], stats: { kept: { runs: 0, size: 0 }, pruned: { runs: 0, size: 0 } } };
        for (let job of jobs) {
            let settings = await fetch_json(job.settings_url);
            if (settings.retention == undefined || settings.retention == "default") { // We only care about jobs that use the defaults that we're changing
                let prune_result = await fetch_json(url_with(job.prune_url, { settings: JSON.stringify(from_retention_state()) }));
                total_results.pruned = total_results.pruned.concat(prune_result.pruned);
                total_results.stats.kept.runs   += prune_result.stats.kept.runs;
                total_results.stats.kept.size   += prune_result.stats.kept.size;
                total_results.stats.pruned.runs += prune_result.stats.pruned.runs;
                total_results.stats.pruned.size += prune_result.stats.pruned.size;
            }
        }
        return total_results;
    }, [jobs, from_retention_state]);

    return jsr([settings_modal, { kind: "global settings", title: 'Settings', save_settings, close_settings },
                retention == null ? [loading] : [React.Fragment,
                                                 [global_retention_settings, { retention, set_retention_path, prune_dry_run }]],
               ]);
}

export function job_settings({job, close_settings}) {
    let [retention, into_retention_state, from_retention_state, set_retention_path] = use_retention_state();
    let [save_state, set_save_state] = React.useState();

    React.useEffect(() => {
        let cancelled = false;
        (async () => {
            let settings = (await fetch_json(job.settings_url));
            if (cancelled) return;
            into_retention_state(settings.retention);
        })();
        return () => cancelled = true;
    }, [job.runs_url]);

    const save_settings = async () => await _fetch(job.settings_url, {
        method: "PUT",
        body: JSON.stringify({ retention: from_retention_state() }),
    });

    const prune_dry_run = async () => await fetch_json(url_with(job.prune_url, { settings: JSON.stringify(from_retention_state()) }));

    return jsr([settings_modal, { kind: "job settings", title: `Job Settings for ${job.user} / ${job.name}`, save_settings, close_settings },
                 retention == null ? [loading] : [React.Fragment,
                                                  [job_retention_settings, { retention, set_retention_path, prune_dry_run }]],
               ]);
}

function job_retention_settings({retention, set_retention_path, prune_dry_run}) {
    return jsr(['div', { className: "retention" },
                ['h2', "Run Retention" ],
                ['select', { className: 'form-select', 'aria-label': 'Use global defaults', value: retention.default ? "global" : "custom",
                             onChange: (e) => set_retention_path("default", e.target.value == "global") },
                 ['option', { value: 'global', selected:'' }, 'Use global defaults'],
                 ['option', { value: 'custom' }, 'Use custom settings']],
                retention.default || core_retention_settings({ retention, set_retention_path, prune_dry_run }),
               ]);
}

function global_retention_settings({retention, set_retention_path, prune_dry_run}) {
    return jsr(['div', { className: "retention" },
                ['h2', "Run Retention" ],
                [core_retention_settings, { retention, set_retention_path, prune_dry_run }],
               ]);
}

const Loading = Symbol("Loading");
function core_retention_settings({retention, set_retention_path, prune_dry_run}) {
    let [prune_dry_run_stats, set_prune_dry_run_stats] = React.useState(undefined);

    let debouncer = use_debounce(2000);

    const do_dry_run = () => {
        debouncer.hit(async () => {
            try {
                set_prune_dry_run_stats(Loading);
                let results = await prune_dry_run();
                set_prune_dry_run_stats(results.stats);
            } catch(e) {
                // ignore these errors
                console.log(`Ignoring ${e}`);
            }
        });
    };

    React.useEffect(() => {
        do_dry_run();
        return () => {};
    }, [JSON.stringify(retention)]);

    return jsr(['div',
                ['div', { className: 'form-check' },
                 ['input', { className: 'form-check-input', type: 'checkbox', value: '', id: 'retention-time-enable', checked: retention.time.enable,
                             onChange: (e) => set_retention_path("time.enable", e.target.checked) }],
                 ['label', { className: 'form-check-label', for: 'retention-time-enable' },
                  ['div', classes('input-group', 'mb-3', retention.time.error && 'has-validation'),
                   ['span', { className: 'input-group-text', id: 'retention-time' }, 'Keep for'],
                   ['input', { type: 'text', 'aria-label': 'Keep for', 'aria-describedby': 'retention-time', placeholder: "Time",
                               ...classes('form-control', retention.time.error && 'is-invalid'),
                               disabled: !retention.time.enable, value: retention.time.value,
                               onChange: (e) => set_retention_path("time.value", e.target.value) }],
                   ['select', { className: 'form-select input-group-text', 'aria-label': '', defaultValue: 'days',
                                disabled: !retention.time.enable, value: retention.time.units,
                                onChange: (e) => set_retention_path("time.units", e.target.value) },
                    ['option', { value: 'd' }, 'Days'],
                    ['option', { value: 'w' }, 'Weeks'],
                    ['option', { value: 'm' }, 'Months'],
                    ['option', { value: 'y' }, 'Years']],
                   retention.time.error && ['div', { className: 'invalid-feedback' }, retention.time.error ]]]],

                ['div', { className: 'form-check' },
                 ['input', { className: 'form-check-input', type: 'checkbox', value: '', id: 'retention-runs-enable', checked: retention.runs.enable,
                             onChange: (e) => set_retention_path("runs.enable", e.target.checked) }],
                 ['label', { className: 'form-check-label', for: 'retention-runs-enable' },
                  ['div', classes('input-group', 'mb-3', retention.time.error && 'has-validation'),
                   ['span', { className: 'input-group-text', id: 'retention-runs' }, '…but no more than'],
                   ['input', { type: 'text', className: 'form-control', 'aria-label': 'Keep for', 'aria-describedby': 'retention-runs', placeholder: "Runs",
                               ...classes('form-control', retention.runs.error && 'is-invalid'),
                               disabled: !retention.runs.enable, value: retention.runs.value,
                               onChange: (e) => set_retention_path("runs.value", e.target.value) }],
                   ['span', { className: "input-group-text" }, "Runs"],
                   retention.runs.error && ['div', { className: 'invalid-feedback' }, retention.runs.error ]]]],

                ['div', { className: 'form-check' },
                 ['input', { className: 'form-check-input', type: 'checkbox', value: '', id: 'retention-size-enable', checked: retention.size.enable,
                             onChange: (e) => set_retention_path("size.enable", e.target.checked) }],
                 ['label', { className: 'form-check-label', for: 'retention-size-enable' },
                  ['div', classes('input-group', 'mb-3', retention.time.error && 'has-validation'),
                   ['span', { className: 'input-group-text', id: 'retention-size' }, '…and not to exceed'],
                   ['input', { type: 'text', className: 'form-control', 'aria-label': 'Keep for', 'aria-describedby': 'retention-size', placeholder: "Size",
                               ...classes('form-control', retention.size.error && 'is-invalid'),
                               disabled: !retention.size.enable, value: retention.size.value,
                               onChange: (e) => set_retention_path("size.value", e.target.value)} ],
                   ['select', { className: 'form-select input-group-text', 'aria-label': '', value: 'mb',
                                disabled: !retention.size.enable, value: retention.size.units,
                                onChange: (e) => set_retention_path("size.units", e.target.value) },
                    ['option', { value: 'b'  }, 'Bytes'],
                    ['option', { value: 'kb' }, 'KB'],
                    ['option', { value: 'mb' }, 'MB'],
                    ['option', { value: 'gb' }, 'GB'],
                    ['option', { value: 'tb' }, 'TB']],
                   retention.size.error && ['div', { className: 'invalid-feedback' }, retention.size.error ]]]],
                ['div', classes('alert', 'alert-info', prune_dry_run_stats == undefined && 'hidden'),
                 prune_dry_run_stats == undefined ? false :
                 prune_dry_run_stats == Loading   ? [loading, { message: "Calculating the effects of your changes…" }]
                                                  : (`Your changes would keep ${prune_dry_run_stats.kept.runs} runs` +
                                                     ` (${human_bytes(prune_dry_run_stats.kept.size)} on disk),` +
                                                     ` and prune ${prune_dry_run_stats.pruned.runs} runs,` +
                                                     ` freeing ${human_bytes(prune_dry_run_stats.pruned.size)} of disk space.`)]
                ]);
}


