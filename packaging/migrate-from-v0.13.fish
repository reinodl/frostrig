#!/usr/bin/env fish
# One-time cleanup of the v0.13 helpers, services and polkit rule.
# Run this BEFORE installing FrostRig, then reboot, then install FrostRig:
# the new service records the laptop's factory fan curve and power limits
# the first time it runs, so nothing old should have changed them that boot.
#
# Your own scripts (gt77-fan-profile, cpu-power-limits) are only disabled,
# never deleted. Their files stay where they are.

set -l old_units msi-control-profile.service msi-control-fans.service msi-control-resume-check.service
set -l your_units gt77-fan-profile.service cpu-power-limits.service

echo "This will:"
echo "  • stop and remove MSI Control v0.13 services: $old_units"
echo "  • disable (not delete) your own startup units: $your_units"
echo "  • remove the v0.13 helpers from /usr/local/libexec and the resume hook"
echo "  • remove the v0.13 per-user polkit rule"
echo "  • keep /var/lib/msi-control/ but move the old files into old-v0.13/"
read -l -P "Continue? [y/N] " answer
if not string match -qi 'y' -- $answer
    echo "Nothing changed."
    exit 1
end

for unit in $old_units $your_units
    if systemctl list-unit-files $unit --no-legend 2>/dev/null | string match -q "*$unit*"
        sudo systemctl disable --now $unit; and echo "Disabled $unit"
    end
end

for unit in $old_units
    sudo rm -f /etc/systemd/system/$unit
end
sudo rm -rf /etc/systemd/system/msi-control-resume-check.service.d
sudo rm -f /usr/lib/systemd/system-sleep/msi-control-profile
sudo rm -f /usr/local/libexec/msi-control-ec-helper /usr/local/libexec/msi-control-power-helper \
    /usr/local/libexec/msi-control-boost-helper /usr/local/libexec/msi-control-battery-helper \
    /usr/local/libexec/msi-control-profile-helper /usr/local/libexec/msi-control-resume-check
sudo rm -f /etc/polkit-1/rules.d/49-msi-control-uid-(id -u).rules
sudo systemctl daemon-reload

if sudo test -d /var/lib/msi-control
    sudo mkdir -p /var/lib/msi-control/old-v0.13
    for f in (sudo find /var/lib/msi-control -mindepth 1 -maxdepth 1 -not -name old-v0.13)
        sudo mv $f /var/lib/msi-control/old-v0.13/
    end
end

echo
echo "Done. Next: reboot, then build and install FrostRig (see README)."
