import { PageMeta } from './page-meta';

export function Privacy() {
  return <section class="wrap page-shell"><article class="prose">
    <PageMeta title="Privacy" canonical="/privacy" description="What Anymd collects when you visit the website, use the local converter, or buy Pro." />
    <h1>Privacy</h1>
    <p>Updated 22 November 2026.</p>
    <p><strong>Sylphx Limited</strong> (company number 16438428, 128 City Road, London EC1V 2NX, United Kingdom) controls the personal data described here. Contact <a href="mailto:hi@sylphx.com">hi@sylphx.com</a> or +44 7537 188 188.</p>
    <h2>Your documents</h2>
    <p>Studio converts files in your browser. It does not upload them. The local CLI and MCP tools do not send usage telemetry. They make network requests when you ask them to fetch a URL, download a model, sign in, or use another network feature.</p>
    <h2>Website and optional cookies</h2>
    <p>Sylphx Hosting and Cloudflare process request details such as your IP address to deliver and protect this site. We check your approximate country through Cloudflare on this site to apply the right cookie defaults. Anymd stores your cookie choice in your browser.</p>
    <p>In the European Economic Area, United Kingdom and Switzerland, or if your region cannot be determined, we do not load Google Analytics or send it requests unless you opt in. In other regions analytics and ad measurement are on by default. Global Privacy Control overrides these settings and keeps optional tracking off wherever you are.</p>
    <p>You can choose <strong>Reject optional</strong>, <strong>Analytics only</strong> or <strong>Allow analytics &amp; ads</strong>. Use <strong>Cookie settings</strong> in the footer to change or withdraw your choice. Rejecting optional tracking stops future measurement on this site; it does not delete data already sent. We disable Google ad personalization for all choices.</p>
    <p>When allowed, Google Analytics 4 measures which sections people visit, visits to pricing, successful starts of hosted checkout, and verified purchases that return to this browser from hosted checkout. To check that payment, the browser keeps a temporary checkout reference in the tab. The reference stays between the browser and Anymd. Purchase measurement includes an opaque transaction identifier, currency and amount. Google may process your IP address, browser and device details, cookie identifiers, and, when ad measurement is enabled, advertising attribution signals. Anymd does not include your name, email, files, full page URL, order receipt link or payment details in analytics events. The site remembers reported transaction identifiers in your browser to avoid counting the purchase again. Read <a href="https://policies.google.com/technologies/partner-sites">how Google uses data from sites that use its services</a>.</p>
    <p>Where opt-in is required we rely on your consent. Elsewhere we use measurement to understand and improve the site and see whether ads work, subject to your choice and applicable law. Optional tracking has no effect on Studio, sign-in or buying Pro.</p>
    <h2>Accounts, Pro and support</h2>
    <p>Sylphx Auth processes your sign-in details to manage your account and session. If you choose Google sign-in, Google helps authenticate you. Sylphx Money and <a href="https://stripe.com/privacy">Stripe</a> process purchases. We use billing and contact details to deliver your licence, manage your account, handle refunds and meet legal obligations. We keep purchase records for six years for tax purposes. If you contact us, we use your message and address to reply and keep them for up to two years after the conversation ends.</p>
    <h2>Your rights</h2>
    <p>You can request access, correction or deletion, or object to processing by emailing <a href="mailto:hi@sylphx.com">hi@sylphx.com</a>. You can complain to the <a href="https://ico.org.uk/">UK Information Commissioner’s Office</a> or your local data protection authority. See the <a href="https://sylphx.com/legal/privacy">Sylphx Privacy Policy</a> for other Sylphx services.</p>
  </article></section>;
}
