# Restrict accounting state ownership and support explicit traffic correction

Only the periodic accounting reset task and an explicit administrator correction command may write accounting state. Normal traffic/status reads and Subscription requests remain read-only. A direction-aware correction may set RX and TX independently; a total-only correction is stored as a separate adjustment and must not fabricate directional counter values. Before the first Anchored-month reset, the deployment reports a valid Pending first reset state with zero usage rather than an error.

This keeps HTTP request handling from mutating monthly state while preserving the operational ability to repair an incorrect starting value or recover historical usage after a counter reset.
