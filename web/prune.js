// Copyright © 2024 David Caldwell <david@porkrind.org>
import { React, jsr } from "./lib/jsml-react-bundle.js"
import { loading, card, classes, prevent_default, human_bytes, progress } from "./utils.js"

export function prune_modal({ prune_state, job, done }) {
    let [show_pruned, set_show_pruned] = React.useState(false);
    return jsr(['div',
                ['div', { className: "backdrop" }],
                [card, { kind: "prune modal", title: job ? `Pruning ${job.user} / ${job.name}`
                                                         : [progress, { current: prune_state.progress?.index, max: prune_state.progress?.max, message: prune_state.progress?.message }]},
                 prune_state?.error && ['div', { className: "error" }, ['h3', "Pruning failed!"], prune_state.error],
                 prune_state?.pruning && [loading, { message: "Pruning, please wait…" }],
                 prune_state?.result && ['div',
                                         ['table', { className: "stats" },
                                          ['tbody',
                                           ['tr', ['th', "Pruned Runs"],    ['td', prune_state.result.stats.pruned.runs]],
                                           ['tr', ['th', "Pruned Size"],    ['td', human_bytes(prune_state.result.stats.pruned.size)]],
                                           ['tr', ['th', "Remaining Runs"], ['td', prune_state.result.stats.kept.runs]],
                                           ['tr', ['th', "Remaining Size"], ['td', human_bytes(prune_state.result.stats.kept.size)]]]],
                                         ['h2', classes("deleted-runs", show_pruned ? "show" : "hide"), "Deleted Runs",
                                          { onClick: prevent_default(() => set_show_pruned(!show_pruned)) }],
                                         ['table',
                                          ['thead', ['tr', ['th', 'Run ID'], ['th', 'Size'], ['th', 'Reason Pruned']]],
                                          ['tbody',
                                           prune_state.result.pruned.length == 0 && ['tr', ['td', { colSpan: 3 }, "No runs pruned"]],
                                           prune_state.result.pruned.map(run => ['tr',
                                                                                 ['td', run.run_id],
                                                                                 ['td', human_bytes(run.size)],
                                                                                 ['td', run.reason]])]]],
                 ['div', { className: "buttons" },
                  ['button', { disabled: prune_state?.pruning, onClick: prevent_default(() => done()) }, 'Ok']],
                ]]);
}
