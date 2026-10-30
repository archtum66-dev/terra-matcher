`cli_settle_args.json` is the output of `terra-keys settle-args` (two orders made with `terra-keys make-order`,
fill of 3 lots at price 95 ticks). The two order keys are throw-away test keys; only their public halves appear in
`cli_interop.rs`. To regenerate the file, see `docs/TESTNET.md`, sections 6 and 7.
