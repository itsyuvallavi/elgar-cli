# bin/_archive

## Purpose

Archived local scripts from the pre-raw-baseline harness.

## Why Archived

Most scripts in this folder expect old capabilities such as:

- `/permissions`
- `/approve` and `/reject`
- `/tool`
- `/memory`
- `/plan`
- shell execution
- verified action state
- trace/performance CLI commands

Those capabilities are paused or archived while Elgar is rebuilt from the raw
chat baseline.

## Rule

Do not run these scripts as current verification. When a feature is rebuilt,
write a new small script or restore one archived script only after updating it
to the current architecture.
