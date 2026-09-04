# Separate VPS refresh and client display timezones

sbctl stores the VPS refresh timezone separately from the client display timezone. New deployments default to America/Los_Angeles for the actual accounting reset and Asia/Shanghai for human-readable client-facing reset times, while both values represent the same absolute reset instant; changing only the display timezone must not reset accounting state. This preserves the operator's preferred VPS accounting schedule without forcing the same timezone on the person reading subscription metadata.
