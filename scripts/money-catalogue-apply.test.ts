import { afterEach, describe, expect, test } from 'bun:test'
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, join } from 'node:path'

const root = join(import.meta.dir, '..')
const product = 'anymd'
const defaultBranch = 'main'
const catalogue = JSON.parse(readFileSync(join(root, 'config/money-catalogue.json'), 'utf8')) as {
	spec: { products: { licence?: { key?: string } }[] }
}
const licenceIds = [
	...new Set(
		catalogue.spec.products
			.map((product) => product.licence?.key)
			.filter((value): value is string => typeof value === 'string' && value.length > 0),
	),
]
const licenceId = licenceIds[0]
const dirs: string[] = []
afterEach(() => {
	for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true })
})

const FAKE_CURL = `#!/usr/bin/env bash
out=""; fmt=""; method="GET"; url=""; data=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -o) out="$2"; shift 2 ;;
    -w) fmt="$2"; shift 2 ;;
    -X) method="$2"; shift 2 ;;
    -H|-d) shift 2 ;;
    --data) data="$2"; shift 2 ;;
    -sS|--fail-with-body) shift ;;
    *) url="$1"; shift ;;
  esac
done
echo "$method $url" >> "$FAKE_LOG"
answer() {
  if [ -n "$out" ]; then printf '%s' "$1" > "$out"; else printf '%s' "$1"; fi
  if [ -n "$fmt" ]; then printf '%s' "$2"; fi
}
case "$url" in
  *audience=sylphx-access*) answer '{"value":"jwt-fixture"}' 200 ;;
  */v1/access/github/token) answer "$FAKE_EXCHANGE_BODY" "$FAKE_EXCHANGE_CODE" ;;
  */merchant_accounts:connect)
    printf '%s' "$data" > "$RUNNER_TEMP/merchant-connect.json"
    if [ "\${FAKE_MERCHANT_CONNECT_CODE:-200}" -lt 300 ]; then touch "$RUNNER_TEMP/connected-merchant"; fi
    if [ -n "$FAKE_MERCHANT_CONNECT_BODY" ]; then answer "$FAKE_MERCHANT_CONNECT_BODY" "\${FAKE_MERCHANT_CONNECT_CODE:-200}"; else answer '{}' "\${FAKE_MERCHANT_CONNECT_CODE:-200}"; fi ;;
  */merchant_accounts/default)
    body='{"livemode":false,"state":"connected","charges_enabled":true}'
    if [ -f "$RUNNER_TEMP/connected-merchant" ]; then
      if [ -n "$FAKE_CONNECTED_MERCHANT_BODY" ]; then body="$FAKE_CONNECTED_MERCHANT_BODY"; fi
      answer "$body" "\${FAKE_CONNECTED_MERCHANT_CODE:-200}"
    elif [ -n "$FAKE_MERCHANT_GET_CODE" ]; then
      if [ -n "$FAKE_MERCHANT_GET_BODY" ]; then body="$FAKE_MERCHANT_GET_BODY"; fi
      answer "$body" "$FAKE_MERCHANT_GET_CODE"
    elif [ -n "$FAKE_MERCHANT_EXISTS" ]; then
      if [ -n "$FAKE_MERCHANT_BODY" ]; then body="$FAKE_MERCHANT_BODY"; fi
      answer "$body" 200
    else
      answer '{"error":"not found"}' 404
    fi ;;
  */licence_keys:generate) touch "$RUNNER_TEMP/generated-licence-key"; answer '{}' 200 ;;
  */licence_keys/*)
    if [ -n "$FAKE_KEY_EXISTS" ] || [ -f "$RUNNER_TEMP/generated-licence-key" ]; then
      answer '{}' 200
    else
      answer '{"error":"not found"}' 404
    fi ;;
  */price_catalogs/default:sync)
    if [ -n "$FAKE_SYNC_BODY" ]; then answer "$FAKE_SYNC_BODY" "\${FAKE_SYNC_CODE:-200}"; else answer '{}' "\${FAKE_SYNC_CODE:-200}"; fi ;;
  */price_catalogs/default*)
    if [ -n "$FAKE_MONEY_BODY" ]; then answer "$FAKE_MONEY_BODY" "\${FAKE_MONEY_CODE:-200}"; else answer '{}' "\${FAKE_MONEY_CODE:-200}"; fi ;;
  *) answer '{"unexpected":true}' 404 ;;
esac
`
const GRANTED = JSON.stringify({
	['api_' + 'key']: 'fixture-api-key',
	key: { name: 'orgs/o/projects/p/envs/e/api_keys/k' },
	active_after_ms: 0,
})

function run(env: Record<string, string>) {
	const dir = mkdtempSync(join(tmpdir(), 'money-catalogue-apply-'))
	dirs.push(dir)
	const curl = join(dir, 'curl')
	writeFileSync(curl, FAKE_CURL)
	chmodSync(curl, 0o755)
	const log = join(dir, 'calls.log')
	writeFileSync(log, '')
	const merchantConnect = join(dir, 'merchant-connect.json')
	const bunDir = process.execPath.replace(/\/[^/]+$/, '')
	const result = Bun.spawnSync(['bash', 'scripts/money-catalogue-apply.sh'], {
		cwd: root,
		env: {
			PATH: [dir, bunDir, '/usr/bin', '/bin', '/usr/local/bin'].join(delimiter),
			ACCESS_NAME: `${product}-money-catalogue-preview`,
			ACTIONS_ID_TOKEN_REQUEST_URL: 'https://token.invalid/request?x=1',
			ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'fixture',
			RUNNER_TEMP: dir,
			FAKE_LOG: log,
			...env,
		},
	})
	return {
		status: result.exitCode,
		stdout: result.stdout.toString(),
		stderr: result.stderr.toString(),
		calls: readFileSync(log, 'utf8').split('\n').filter(Boolean),
		merchantConnect: existsSync(merchantConnect)
			? JSON.parse(readFileSync(merchantConnect, 'utf8'))
			: undefined,
	}
}

describe('money catalogue apply script', () => {
	test('reuses or creates declared keys, then patches and syncs only the real catalogue', () => {
		const result = run({ FAKE_EXCHANGE_BODY: GRANTED, FAKE_EXCHANGE_CODE: '200' })
		expect(result.status).toBe(0)
		expect(result.calls.filter((call) => call.includes('licence_keys:generate'))).toHaveLength(
			licenceIds.length,
		)
		if (licenceId) {
			expect(result.calls.filter((call) => call.includes(`/licence_keys/${licenceId}`))).toHaveLength(2)
		} else {
			expect(result.calls.some((call) => call.includes('licence_keys'))).toBe(false)
		}
		expect(result.calls.at(-2)).toBe(
			'PATCH https://api.sylphx.com/v1/orgs/o/projects/p/envs/e/price_catalogs/default?allow_missing=true',
		)
		expect(result.calls.at(-1)).toBe(
			'POST https://api.sylphx.com/v1/orgs/o/projects/p/envs/e/price_catalogs/default:sync',
		)
		expect(result.calls.some((call) => call.includes('merchant_accounts'))).toBe(false)
	})

	test('an existing licence key is reused without generating', () => {
		if (!licenceId) return
		const result = run({
			FAKE_EXCHANGE_BODY: GRANTED,
			FAKE_EXCHANGE_CODE: '200',
			FAKE_KEY_EXISTS: '1',
		})
		expect(result.status).toBe(0)
		expect(result.calls.filter((call) => call.includes('licence_keys:generate'))).toEqual([])
		expect(result.calls.filter((call) => call.includes(`/licence_keys/${licenceId}`))).toHaveLength(1)
	})

	test('a missing preview merchant is connected once with the test key and read back', () => {
		const result = run({
			FAKE_EXCHANGE_BODY: GRANTED,
			FAKE_EXCHANGE_CODE: '200',
			STRIPE_TEST_RESTRICTED_KEY: 'rk_test_fixture',
		})
		expect(result.status).toBe(0)
		expect(result.merchantConnect).toEqual({ restricted_key: 'rk_test_fixture' })
		expect(result.calls.filter((call) => call.includes('merchant_accounts'))).toEqual([
			'GET https://api.sylphx.com/v1/orgs/o/projects/p/envs/e/merchant_accounts/default',
			'POST https://api.sylphx.com/v1/orgs/o/projects/p/envs/e/merchant_accounts:connect',
			'GET https://api.sylphx.com/v1/orgs/o/projects/p/envs/e/merchant_accounts/default',
		])
		const readback = result.calls.findLastIndex((call) => call.includes('/merchant_accounts/default'))
		const patch = result.calls.findIndex((call) => call.startsWith('PATCH'))
		expect(readback).toBeGreaterThan(-1)
		expect(readback).toBeLessThan(patch)
	})

	test('an existing connected test merchant is reused without reconnecting', () => {
		const result = run({
			FAKE_EXCHANGE_BODY: GRANTED,
			FAKE_EXCHANGE_CODE: '200',
			FAKE_MERCHANT_EXISTS: '1',
			STRIPE_TEST_RESTRICTED_KEY: 'rk_test_fixture',
		})
		expect(result.status).toBe(0)
		expect(result.merchantConnect).toBeUndefined()
		expect(result.calls.filter((call) => call.includes('merchant_accounts'))).toEqual([
			'GET https://api.sylphx.com/v1/orgs/o/projects/p/envs/e/merchant_accounts/default',
		])
	})

	test('a live, disconnected or charge-disabled merchant is never reused', () => {
		for (const body of [
			'{"livemode":true,"state":"connected","charges_enabled":true}',
			'{"livemode":false,"state":"disconnected","charges_enabled":true}',
			'{"livemode":false,"state":"connected","charges_enabled":false}',
		]) {
			const result = run({
				FAKE_EXCHANGE_BODY: GRANTED,
				FAKE_EXCHANGE_CODE: '200',
				FAKE_MERCHANT_EXISTS: '1',
				FAKE_MERCHANT_BODY: body,
				STRIPE_TEST_RESTRICTED_KEY: 'rk_test_fixture',
			})
			expect(result.status).not.toBe(0)
			expect(result.calls.some((call) => call.includes('merchant_accounts:connect'))).toBe(false)
			expect(result.calls.some((call) => call.includes('price_catalogs') || call.includes('licence_keys'))).toBe(false)
		}
	})

	test('a non-test key is refused before any Money call', () => {
		const result = run({
			FAKE_EXCHANGE_BODY: GRANTED,
			FAKE_EXCHANGE_CODE: '200',
			STRIPE_TEST_RESTRICTED_KEY: 'rk_live_fixture',
		})
		expect(result.status).not.toBe(0)
		expect(result.calls.some((call) => call.includes('/v1/orgs/'))).toBe(false)
	})

	test('merchant read, connect and readback failures stop before catalogue writes', () => {
		for (const env of [
			{ FAKE_MERCHANT_GET_CODE: '500' },
			{ FAKE_MERCHANT_CONNECT_CODE: '500' },
			{ FAKE_CONNECTED_MERCHANT_CODE: '500' },
			{ FAKE_CONNECTED_MERCHANT_BODY: '{"livemode":true,"state":"connected","charges_enabled":true}' },
		] as Record<string, string>[]) {
			const result = run({
				FAKE_EXCHANGE_BODY: GRANTED,
				FAKE_EXCHANGE_CODE: '200',
				STRIPE_TEST_RESTRICTED_KEY: 'rk_test_fixture',
				...env,
			})
			expect(result.status).not.toBe(0)
			expect(result.calls.filter((call) => call.includes('merchant_accounts:connect'))).toHaveLength(
				env['FAKE_MERCHANT_GET_CODE'] ? 0 : 1,
			)
			expect(result.calls.some((call) => call.includes('price_catalogs') || call.includes('licence_keys'))).toBe(false)
		}
	})

	test('a refused exchange stops before any Money write', () => {
		const result = run({ FAKE_EXCHANGE_BODY: '{"error":"refused"}', FAKE_EXCHANGE_CODE: '403' })
		expect(result.status).not.toBe(0)
		expect(result.calls).toEqual([
			'GET https://token.invalid/request?x=1&audience=sylphx-access',
			'POST https://api.sylphx.com/v1/access/github/token',
		])
	})

	test('a failed PATCH does not sync', () => {
		const result = run({
			FAKE_EXCHANGE_BODY: GRANTED,
			FAKE_EXCHANGE_CODE: '200',
			FAKE_MONEY_CODE: '500',
		})
		expect(result.status).not.toBe(0)
		expect(result.calls.some((call) => call.includes(':sync'))).toBe(false)
	})

	test('only a keyless preview with the sole merchant_not_connected reason may continue', () => {
		const merchantMissing = JSON.stringify({
			code: 'INVALID_STATE',
			details: [{ reason: 'merchant_not_connected' }],
		})
		const base = {
			FAKE_EXCHANGE_BODY: GRANTED,
			FAKE_EXCHANGE_CODE: '200',
			FAKE_SYNC_BODY: merchantMissing,
			FAKE_SYNC_CODE: '400',
			SYNC_FAILURE_NONFATAL: '1',
		}
		const missing = run(base)
		expect(missing.status, missing.stdout + missing.stderr).toBe(0)
		for (const env of [
			{ FAKE_SYNC_BODY: merchantMissing, STRIPE_TEST_RESTRICTED_KEY: 'rk_test_fixture', FAKE_MERCHANT_EXISTS: '1' },
			{
				FAKE_SYNC_BODY: JSON.stringify({
					code: 'INVALID_STATE',
					details: [{ reason: 'merchant_not_connected' }, { reason: 'different_failure' }],
				}),
			},
			{ FAKE_SYNC_BODY: JSON.stringify({ code: 'INVALID_STATE', details: [{ reason: 'different_failure' }] }) },
			{ FAKE_SYNC_BODY: '{"code":"INTERNAL"}', FAKE_SYNC_CODE: '500' },
		] as Record<string, string>[]) {
			expect(run({ ...base, ...env }).status).not.toBe(0)
		}
	})
})

describe('money catalogue workflow', () => {
	const workflow = Bun.YAML.parse(readFileSync(join(root, '.github/workflows/money-catalogue.yml'), 'utf8')) as {
		on: { push: { branches: string[]; paths: string[] }; pull_request: { paths: string[] } }
		jobs: Record<
			string,
			{
				needs?: string
				if?: string
				env?: Record<string, string>
				permissions?: Record<string, string>
				steps: { run?: string }[]
			}
		>
	}

	const testJob = workflow.jobs['test']!
	const previewJob = workflow.jobs['preview']!
	const productionJob = workflow.jobs['production']!

	test('offline tests run on PRs; writes run only after tests on the default branch', () => {
		expect(workflow.on.push.branches).toEqual([defaultBranch])
		for (const path of [
			'config/money-catalogue.json',
			'scripts/money-catalogue-apply.sh',
		]) {
			expect(workflow.on.push.paths).toContain(path)
			expect(workflow.on.pull_request.paths).toContain(path)
		}
		expect(testJob.permissions?.['id-token']).toBeUndefined()
		expect(testJob.steps.map((step) => step.run).filter(Boolean)).toEqual([
			'bun test scripts/money-catalogue-apply.test.ts',
			'python3 scripts/money-access-verify.test.py',
		])
		expect(previewJob.needs).toBe('test')
		expect(productionJob.needs).toBe('preview')
		for (const job of [previewJob, productionJob]) {
			expect(job.if).toBe(`github.event_name != 'pull_request' && inputs.verify_only != true && github.ref == 'refs/heads/${defaultBranch}'`)
			expect(job.steps.map((step) => step.run).filter(Boolean)).toEqual([
				'bash scripts/money-catalogue-apply.sh',
			])
		}
	})

	test('only preview receives the test key and the exact no-key fallback', () => {
		expect(previewJob.env?.['ACCESS_NAME']).toBe(`${product}-money-catalogue-preview`)
		expect(productionJob.env?.['ACCESS_NAME']).toBe(`${product}-money-catalogue-production`)
		expect(previewJob.env?.['SYNC_FAILURE_NONFATAL']).toBe('1')
		expect(productionJob.env?.['SYNC_FAILURE_NONFATAL']).toBeUndefined()
		expect(previewJob.env?.['STRIPE_TEST_RESTRICTED_KEY']).toBe(
			`\${{ secrets.STRIPE_TEST_RESTRICTED_KEY }}`,
		)
		expect(productionJob.env?.['STRIPE_TEST_RESTRICTED_KEY']).toBeUndefined()
	})
})
