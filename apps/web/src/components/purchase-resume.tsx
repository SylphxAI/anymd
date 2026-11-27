import { A } from '@solidjs/router';
import { ArrowLeft } from 'lucide-solid';
import type { Locale } from '@/lib/marketing-locale';
import type { PurchaseResume } from '@/lib/purchase-resume';

const cliLogin = 'anymd login';

// Read-only reminder of the purchase in progress. Nothing here starts checkout:
// the buyer returns to pricing, confirms the licence terms and continues there.
export function PurchaseSummary(props: { purchase: PurchaseResume; locale: Locale }) {
  const zh = () => props.locale === 'zh-TW';
  return <section class="panel stack purchase-resume" aria-labelledby="purchase-resume-title">
    <span class="eyebrow">{zh() ? '你的購買' : 'YOUR PURCHASE'}</span>
    <h2 id="purchase-resume-title">Anymd Pro · {props.purchase.price}</h2>
    <p class="muted">{zh()
      ? `以 ${props.purchase.currency} 一次付清，價格已含稅，不必訂閱。`
      : `One-time payment in ${props.purchase.currency}. Tax included. No subscription.`}</p>
    <p>{zh()
      ? '登入後會回到價格頁面。確認授權條款並按下「前往安全結帳」前，不會收取任何費用。'
      : 'After you sign in, you return to pricing. Nothing is charged until you confirm the licence terms and choose Continue to secure checkout.'}</p>
    <p>{zh() ? '付款後，授權會連結到這個帳號。在你的電腦執行 ' : 'After payment, your licence is linked to this account. Run '}<code>{cliLogin}</code>{zh() ? ' 即可啟用 Pro。' : ' on your machine to activate Pro.'}</p>
    <A class="inline-link inline-flex items-center gap-1 whitespace-nowrap justify-self-start" href={props.purchase.query ? props.purchase.path + '?' + props.purchase.query : props.purchase.path}><ArrowLeft size={15} aria-hidden="true" class="shrink-0" />{zh() ? '返回價格頁面' : 'Back to pricing'}</A>
  </section>;
}
