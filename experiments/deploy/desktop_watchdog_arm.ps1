# Self-healing relaunch for ONE desktop TTA replicate arm, selected by -K.
#
# SUPERSEDED (2026-08-06) -- this is now a no-op stub, not a launcher.
#
# The three-arm CPU league (2p/3p/4p) that this script used to drive now
# runs exclusively on the Mac mini via experiments/rust_league.sh + cron,
# writing directly to the shared, git-tracked
# experiments/rust_champion_{2,3,4}p.json checkpoints. Reviving a SECOND set
# of arms here, on the desktop, against those same paths would be two
# machines racing to write one file, not a second worker on the same job --
# so there is no drop-in Rust replacement to launch from this script.
#
# What this script used to do: shell out to ./experiments/desktop_arm.sh,
# which does not exist any more, with flags for a Python league that is also
# gone (`--candidate-bot quiescent:levels=1` was a Python bot spec;
# `--hall-dir experiments/hall_of_fame` pointed at a directory nothing
# creates today). That made every 15-minute invocation a guaranteed failure
# to launch anything, silently, which is worse than an honest no-op.
#
# Why this file still exists instead of being deleted: register_tasks.ps1
# still schedules three Scheduled Tasks (TTA_Arm_2p_Interval,
# TTA_Arm_3p_Interval, TTA_Arm_4p_Interval) that invoke this script by path
# every 15 minutes. Deleting it would turn each of those into the exact
# failure mode register_tasks.ps1's own header warns about for a task
# pointing at a script that no longer exists: it "would fire every five
# minutes forever, fail every time, and leave a trail of Last Result errors
# that looks exactly like a guard that is broken rather than one that was
# retired on purpose." Keeping a clean no-op here is that same fix, applied
# at the script rather than the task-registration layer (register_tasks.ps1
# is out of scope for this change; deregistering the three tasks belongs
# there, next time that file is touched).
#
# See experiments/rust_league.sh's own header for the design of what
# replaced this (a Rust `climb` process per player count, resumable from its
# own checkpoint, no separate flags to re-supply on relaunch) and
# docs/NEURAL.md (Operating the shared desktop box) for the desktop's own
# deploy notes.
#
# The design notes below, about why aliveness was judged by log mtime rather
# than process enumeration and why relaunches reaped the previous driver by
# stored PID, are kept as historical background: they explain scars in the
# Scheduled Task setup (LogonTrigger, Hidden, the reap-by-PID pattern
# elsewhere in this repo) that are still accurate even though this
# particular script no longer launches anything.
#
# - Split into one task per arm (rather than one script looping over all
#   three) because looping Start-Process calls for 2p/3p/4p from a single
#   parent PowerShell process was observed to reliably hang the 1st and 3rd
#   launch while the 2nd succeeded, regardless of which player-count was in
#   which position -- looks like contention in process creation from a single
#   parent hitting something (COM/shell init?) that's slow once, fast right
#   after, contended on a third close call. Three independent Scheduled Tasks
#   each doing exactly one Start-Process call sidestepped it entirely.
# - Aliveness was judged by log mtime (not process enumeration --
#   Get-CimInstance Win32_Process from a Scheduled Task context returned zero
#   matches for processes confirmed alive seconds earlier by a direct check).
# - Relaunches reaped the previous driver tree by stored PID
#   (`taskkill /F /T`) before starting a new one, so a leaked driver could
#   not outlive its replacement.

param([Parameter(Mandatory=$true)][int]$K)

$watchdogLog = "C:\Users\micro\tta_watchdog.log"
"$(Get-Date -Format o) watchdog(${K}p): superseded by experiments/rust_league.sh on the Mac mini -- no-op" | Out-File -Append $watchdogLog
exit
