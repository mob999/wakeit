#!/usr/bin/env python3
"""Protect job selection and the required aggregate check, without running suites."""
import copy
from pathlib import Path
import subprocess
import tempfile
import unittest
from ci_scope import JOBS, changed_paths, check_results, select_scopes


class ScopeChecks(unittest.TestCase):
    def selected(self, *paths):
        return {name for name, enabled in select_scopes(paths).items() if enabled}

    def test_frontend_and_docs_do_not_start_backend_jobs(self):
        self.assertEqual(self.selected('web/src/tasks/TaskForm.tsx'), {'frontend', 'web'})
        self.assertEqual(self.selected('web/package-lock.json'), {'frontend', 'web', 'packaging'})
        self.assertEqual(self.selected('docs/operations.md', 'CHANGELOG.md'), {'docs'})

    def test_backend_shared_contracts_and_installation_keep_their_coverage(self):
        self.assertEqual(self.selected('crates/wakeit-cli/src/runner/mod.rs'), {'cli', 'integration', 'web'})
        self.assertEqual(self.selected('scripts/integration.py'), {'integration', 'web'})
        self.assertEqual(self.selected('schemas/task.v1.json'),
                         {'frontend', 'cli', 'gateway', 'core', 'integration', 'web', 'docs', 'packaging'})
        self.assertEqual(self.selected('install.sh'), {'installer', 'packaging'})
        self.assertEqual(self.selected('deploy-gateway.sh'), {'installer', 'packaging'})
        self.assertEqual(self.selected('crates/wakeit-gateway/src/gateway.rs'), {'gateway', 'integration', 'web'})
        self.assertEqual(self.selected('crates/wakeit-core/src/protocol.rs'),
                         {'cli', 'gateway', 'core', 'integration', 'web'})
        self.assertEqual(self.selected('skills/wakeit/SKILL.md'), {'docs', 'packaging'})
        self.assertEqual(self.selected('skills/wakeit/assets/gateway.yaml'),
                         {'cli', 'gateway', 'core', 'integration', 'web', 'docs', 'packaging'})
        self.assertEqual(self.selected('web/src/main.tsx', 'crates/wakeit-gateway/src/api/tasks.rs'),
                         {'frontend', 'gateway', 'integration', 'web'})

    def test_package_tests_do_not_start_service_integration(self):
        self.assertEqual(self.selected('crates/wakeit-cli/tests/sessions.rs'), {'cli'})
        self.assertEqual(self.selected('crates/wakeit-core/tests/task_templates.rs'), {'core'})
        # Gateway tests still need assets for the embedded WebUI.
        self.assertEqual(self.selected('crates/wakeit-gateway/tests/preview.rs'), {'gateway', 'web'})

    def test_ci_unknown_paths_and_manual_runs_select_everything(self):
        full = {name for name, enabled in select_scopes(None).items() if enabled}
        self.assertEqual(self.selected('.github/workflows/verify.yml'), full)
        self.assertEqual(self.selected('new-code/entry.py'), full)
        self.assertEqual(changed_paths('workflow_dispatch', {}), None)
        self.assertEqual(changed_paths('push', {'before': '0' * 40}), None)
        self.assertEqual(self.selected(), set())

    def test_required_result_rejects_failed_cancelled_and_missing_selected_jobs(self):
        scopes = select_scopes(['web/src/main.tsx'])
        needs = {'scope': {'result': 'success', 'outputs': {k: str(v).lower() for k, v in scopes.items()}}}
        needs.update({job: {'result': 'success' if scopes[job] else 'skipped'} for job in JOBS})
        check_results(needs)
        for result in ('failure', 'cancelled', 'skipped', None):
            with self.subTest(result=result):
                changed = copy.deepcopy(needs)
                changed['web']['result'] = result
                with self.assertRaises(ValueError):
                    check_results(changed)
        for key in ('scope', 'web'):
            changed = copy.deepcopy(needs)
            del changed[key]
            with self.assertRaises(ValueError):
                check_results(changed)
        changed = copy.deepcopy(needs)
        changed['scope']['outputs'].pop('gateway')
        with self.assertRaises(ValueError):
            check_results(changed)

    def test_git_diff_uses_pr_merge_base_and_keeps_deleted_and_renamed_paths(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            def git(*args):
                return subprocess.check_output(['git', *args], cwd=root, text=True).strip()
            def commit(message):
                git('add', '-A')
                git('commit', '-qm', message)
                return git('rev-parse', 'HEAD')
            git('init', '-q')
            git('config', 'user.name', 'Scope fixture')
            git('config', 'user.email', 'fixture@example.invalid')
            (root / 'web').mkdir()
            (root / 'docs').mkdir()
            (root / 'web/old.ts').write_text('old')
            (root / 'web/move.ts').write_text('move')
            original = commit('initial')
            git('checkout', '-qb', 'feature')
            (root / 'web/old.ts').unlink()
            (root / 'web/move.ts').rename(root / 'docs/moved.md')
            head = commit('feature')
            git('checkout', '-q', original)
            (root / 'backend.rs').write_text('base-only change')
            base = commit('base advanced')
            paths = {'web/old.ts', 'web/move.ts', 'docs/moved.md'}
            self.assertEqual(set(changed_paths('pull_request', {
                'pull_request': {'base': {'sha': base}, 'head': {'sha': head}},
            }, root)), paths)
            self.assertEqual(set(changed_paths('push', {'before': original, 'after': head}, root)), paths)


if __name__ == '__main__':
    unittest.main()
