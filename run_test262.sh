cargo test test262_runner_main -- --nocapture | tee run.log
echo SUMMARY
tail -n 10 run.log
