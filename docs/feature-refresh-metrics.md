# Feature refresh alerts

Edge initializes warning and refresh-error counters at zero before work starts
for an environment. This lets Prometheus observe the first increase after a
healthy scrape. Initialization is idempotent and never resets existing counts.

A failure during startup can precede the first scrape. `increase` alone cannot
detect that initial nonzero value. To also alert when a nonzero series has
appeared within the last 15 minutes, use:

```promql
(increase(edge_feature_state_warnings_total[15m]) > 0)
or
((edge_feature_state_warnings_total > 0)
 unless edge_feature_state_warnings_total offset 15m)
```

```promql
(increase(edge_feature_refresh_errors_total[15m]) > 0)
or
((edge_feature_refresh_errors_total > 0)
 unless edge_feature_refresh_errors_total offset 15m)
```

Keep instance labels when evaluating these expressions so that a healthy node
does not hide a failed node. Newly discovered nonzero series alert for the window
even if the failure occurred before monitoring discovered the process. As with
other scraped counters, failures in processes that exit before any scrape cannot
be recovered from these metrics.

`edge_last_applied_revision_id` advances after the engine update completes.
Compilation warnings can discard individual toggles while still applying the
remaining state, so use the warning counter alongside the revision gauge.
