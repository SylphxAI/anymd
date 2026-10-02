"""scripts/npm-settle.sh retries registry delays only, with a bound."""
import os
import stat
import subprocess
import tempfile
import unittest

SCRIPT = os.path.join(os.path.dirname(__file__), "npm-settle.sh")


def run(fake_body, bound="3"):
    with tempfile.TemporaryDirectory() as tmp:
        fake = os.path.join(tmp, "check")
        with open(fake, "w") as f:
            f.write("#!/usr/bin/env bash\n" + fake_body)
        os.chmod(fake, os.stat(fake).st_mode | stat.S_IEXEC)
        env = dict(os.environ, SETTLE_BOUND=bound, SETTLE_FIRST="1", SETTLE_MAX="1", TMPDIR=tmp)
        return subprocess.run(
            ["bash", "-c", f'source "{SCRIPT}"; settle probe "{fake}"'],
            env=env, capture_output=True, text=True, timeout=30,
        )


COUNTER = 'n=$(cat "$TMPDIR/n" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$TMPDIR/n"\n'


class SettleTest(unittest.TestCase):
    def test_retries_until_the_version_is_served(self):
        r = run(COUNTER + 'if [ "$n" -lt 3 ]; then echo "npm error code ETARGET"; exit 1; fi; echo 8.4.0')
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout.count("wait: probe"), 2)
        self.assertIn("ok: probe (attempt 3", r.stdout)

    def test_fails_clearly_after_the_bound(self):
        r = run('echo "No matching version found"; exit 1', bound="2")
        self.assertEqual(r.returncode, 1)
        self.assertIn("still not served by the npm registry", r.stderr)

    def test_other_failures_do_not_wait(self):
        r = run('echo "boom"; exit 7')
        self.assertEqual(r.returncode, 7)
        self.assertNotIn("wait:", r.stdout)


if __name__ == "__main__":
    unittest.main()
