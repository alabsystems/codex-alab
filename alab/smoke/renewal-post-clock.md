# Post-Signal continuation observation budget

The real m5 post-Signal run began near monotonic 292170s. Two legitimate policy
checks delayed Ready to 292291s. Its unchanged native deadline was 292411.937830s.
The controller held through expiry and observed retained source death, settled
input and a quiescent no-Go witness at 292413.214827s. After the hold lifted, a
new generation-one operation attached and executed at approximately fixture
243.3s and resumed at 243.95s; the durable root subsequently became Committed.

The successor's first renewal Status at approximately fixture 244.75s still
returned Waiting. Its following policy checks occurred approximately at 245s,
249s and 257s. The next legitimate exponential policy recheck falls near 273s,
after the previous 270s controller deadline. The old failed run therefore does
not establish a continuation failure or full successful recovery.

Post-Signal cases now allow 390s inside a 420s guardian. This covers two initial
60s policy checks, the real 121s held interval, a successor 60s policy recheck,
and 89s startup/observation headroom, plus 30s bounded cleanup. The production
120s native budget, immutable original receipt/deadline, fresh admission rules
and all causal/model/Consumed assertions are unchanged. Grace remains 450/480s;
ordinary cases remain 150/300s. A fresh actual positive case is still required.
