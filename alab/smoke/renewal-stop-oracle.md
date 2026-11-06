# Pre-Signal owner cancellation

The native renewal service now durably revokes an explicit owner Stop before
resuming its coordinator. A coordinator paused after private Prepare can therefore
exit without sending either a native Signal or Abort RPC. Requiring those RPCs
confuses an old transport sequence with the actual cancellation contract.

The `owner-stop` case keeps the real production Stop gesture and live-vendor
signal, then requires the exact logical intent's durable cancellation with a newer
person sequence. The original native Prepare and expiry remain unchanged. The
coordinator must positively finish before that original deadline. A subsequent
actual native observation must show the exact operation completed rollback-safe,
no native Signal/Resume, no child, and settled input custody, still before expiry.
No native signal syscall, launcher operation or model request may follow.

The original m2 owner-stop run had a successful actual Stop, exact revoked terminal
receipt, and a coordinator exit roughly 119.7 seconds before its deadline. It had
no native action or physical-signal trace. Replaying those existing records passes
the durable cancellation and live-budget checks. That historical run did not
request a native observation, so it does not qualify the new complete oracle;
a fresh real case is required.
