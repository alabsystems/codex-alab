# Grace fixture clock budget

The actual `rn/m3/grace-beyond-budget` run never reached the second grace boundary:

- Native Ready: 292051977323 microseconds.
- Actual person gesture: 292052070801 microseconds.
- Required 121-second grace completion: 292173070801 microseconds.
- Last status before detector cleanup: 292112189973 microseconds.

The old 270-second controller budget included startup's 121-second grace and
policy rechecks, leaving about 60 seconds after the gesture. The run stopped
before the old native deadline or the required second grace completed; it did
not demonstrate a production recovery failure.

Only this fixture case now has a 450-second inner deadline and 480-second guardian:
242 seconds for both real grace intervals, 120 seconds for two legitimate policy
rechecks/reseals, and 88 seconds for startup/restart/observation, with a further
30-second guardian cleanup margin. Other cases retain their existing deadlines.
The production native operation budget remains 120 seconds. Passing still requires
positive old retirement and a fresh distinct operation with preserved original work.
