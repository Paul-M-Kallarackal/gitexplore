import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';

const read = (path) => readFileSync(path, 'utf8');
const json = (path) => JSON.parse(read(path));
const failures = [];
const requireContract = (condition, message) => {
  if (!condition) failures.push(message);
};

const manifest = json('ribbon.json');
const workspace = json('package.json');
const vercel = json('vercel.json');
const dockerfile = read('Dockerfile.vercel');
const release = read('.github/workflows/release.yml');
const schema = read('docker/neo4j/init/01-schema.cypher').replace(/\r\n/g, '\n');

requireContract(manifest.manifestVersion === 2, 'Ribbon manifestVersion must be 2');
requireContract(manifest.productionReadiness?.ready === true, 'Production readiness must be true');
requireContract(
  Array.isArray(manifest.productionReadiness?.blockers) &&
    manifest.productionReadiness.blockers.length === 0,
  'Production blockers must be an empty array'
);
requireContract(['local', 'production'].includes(manifest.status), 'Ribbon status must be local or production');
requireContract(manifest.providers?.deployment?.provider === 'vercel', 'Deployment provider must be Vercel');
requireContract(manifest.providers?.deployment?.mode === 'services', 'Deployment mode must be Vercel Services');
requireContract(
  manifest.providers?.deployment?.hostname === 'gitexplore.moriatz.com',
  'Production hostname must be gitexplore.moriatz.com'
);
requireContract(
  manifest.providers?.backend?.identityStore?.provider === 'neo4j' &&
    manifest.providers.backend.identityStore.productionReady === true &&
    manifest.providers.backend.identityStore.durableOAuthState === true &&
    manifest.providers.backend.identityStore.durableSessions === true,
  'Backend identity must be durable in Neo4j'
);
requireContract(
  manifest.providers?.backend?.identityStore?.encryption?.algorithm === 'xchacha20-poly1305',
  'Identity encryption must use XChaCha20-Poly1305'
);
requireContract(
  manifest.providers?.backend?.refreshCoordination?.provider === 'neo4j-lease',
  'Refresh coordination must use Neo4j leases'
);
requireContract(manifest.providers?.graph?.provider === 'neo4j-aura', 'Graph provider must be Neo4j Aura');
requireContract(
  manifest.providers?.graph?.migrationMode === 'release-gated-idempotent-cli',
  'Neo4j migrations must remain release-gated'
);
requireContract(
  manifest.providers?.auth?.provider === 'github-oauth' &&
    manifest.providers.auth.callbackPath === '/auth/oauth/callback',
  'GitHub OAuth callback contract is missing'
);
requireContract(
  manifest.providers?.designSystem?.commit === workspace.designSystem?.commit &&
    manifest.providers.designSystem.version === workspace.designSystem?.version,
  'Ribbon and package.json must pin the same Strawn version and commit'
);

requireContract(vercel.services?.web?.framework === 'sveltekit', 'Vercel web service must use SvelteKit');
requireContract(
  vercel.git?.deploymentEnabled === false,
  'Automatic Vercel Git deployments must stay disabled; the protected release workflow owns production'
);
requireContract(
  vercel.services?.api?.entrypoint === 'Dockerfile.vercel' &&
    vercel.services.api.runtime === 'container',
  'Vercel API service must use Dockerfile.vercel'
);
requireContract(
  vercel.services?.web?.bindings?.some(
    (binding) =>
      binding.type === 'service' &&
      binding.service === 'api' &&
      binding.env === 'GITEXPLORE_INTERNAL_API_BASE_URL'
  ),
  'Web service must have a private API service binding'
);
for (const route of ['/auth/(.*)', '/graphql', '/health']) {
  requireContract(
    vercel.rewrites?.some((rewrite) => rewrite.source === route && rewrite.destination?.service === 'api'),
    `Vercel API rewrite missing for ${route}`
  );
}
for (const header of ['Strict-Transport-Security', 'X-Content-Type-Options', 'X-Frame-Options']) {
  requireContract(
    vercel.headers?.some((entry) => entry.headers?.some((item) => item.key === header)),
    `Security header ${header} is missing`
  );
}

for (const fragment of [
  'cargo build --locked --release',
  'GITEXPLORE_DEPLOYMENT_MODE=production',
  'GITEXPLORE_GRAPH_BACKEND=neo4j',
  'GITEXPLORE_NEO4J_MAX_TOTAL_NODES=190000',
  'GITEXPLORE_NEO4J_MAX_TOTAL_RELATIONSHIPS=380000',
  'USER gitexplore'
]) {
  requireContract(dockerfile.includes(fragment), `Dockerfile.vercel must contain: ${fragment}`);
}
requireContract(!dockerfile.includes('RUN --mount='), 'Production Dockerfile must work without BuildKit-only RUN mounts');

const requiredServerEnv = [
  'GITEXPLORE_DEPLOYMENT_MODE',
  'GITEXPLORE_FRONTEND_ORIGIN',
  'GITEXPLORE_GITHUB_CLIENT_ID',
  'GITEXPLORE_GITHUB_CLIENT_SECRET',
  'GITEXPLORE_GITHUB_REDIRECT_URI',
  'GITEXPLORE_GRAPH_BACKEND',
  'GITEXPLORE_IDENTITY_ENCRYPTION_KEY',
  'GITEXPLORE_NEO4J_URI',
  'GITEXPLORE_NEO4J_USERNAME',
  'GITEXPLORE_NEO4J_PASSWORD',
  'GITEXPLORE_NEO4J_DATABASE',
  'GITEXPLORE_NEO4J_MAX_TOTAL_NODES',
  'GITEXPLORE_NEO4J_MAX_TOTAL_RELATIONSHIPS'
];
for (const name of requiredServerEnv) {
  requireContract(manifest.runtimeEnvironment?.server?.includes(name), `Ribbon runtime contract is missing ${name}`);
}
requireContract(
  manifest.runtimeEnvironment?.public?.includes('PUBLIC_GITEXPLORE_API_BASE_URL'),
  'Ribbon runtime contract is missing the public API origin'
);

requireContract(/push:\s*\n\s*branches:\s*\[main\]/.test(release), 'Release workflow must deploy only pushes to main');
requireContract(!/^\s*workflow_dispatch:/m.test(release), 'Release workflow must not dispatch arbitrary refs');
requireContract(release.includes('group: production-gitexplore'), 'Production deployments must share one concurrency group');
requireContract(release.includes('pnpm exec vercel deploy --prebuilt --prod'), 'Release must deploy the verified prebuilt artifact');
requireContract(release.includes('vercel inspect') && release.includes('--wait'), 'Release must wait for Vercel readiness');
requireContract(
  release.includes('.graph_backend == "neo4j"') && release.includes('https://gitexplore.moriatz.com/health'),
  'Release must verify the canonical Neo4j-backed health endpoint'
);

const expectedSchemaChecksum = '200b4087d22e330c1b18965083eaddf5b6a8d89982654ecef6b17ff11e8db4f0';
const schemaChecksum = createHash('sha256').update(schema).digest('hex');
requireContract(schemaChecksum === expectedSchemaChecksum, 'Immutable Neo4j v1 schema checksum changed');

if (failures.length > 0) {
  for (const failure of failures) console.error(`FAIL ${failure}`);
  process.exit(1);
}

console.log('Production contract verified');
