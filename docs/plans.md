# Plans: Free and Pro

Novero Mail comes in two plans.

| | Free | Pro |
|---|---|---|
| Mail accounts | 2 | Unlimited |
| Reading, writing, search, folders, local calendar | Yes | Yes |
| Google and Outlook calendar sync | | Yes |
| Rules | | Yes |
| Send later and snooze | | Yes |
| Templates | | Yes |
| Signatures per account | 1 | Unlimited |

Pro is a **one-time purchase**. A license key unlocks Pro for good in every
version released within a year of buying it. Later versions run as Free until
a new key is bought; older ones keep Pro.

New installs get a 14 day trial of Pro. On Free, accounts beyond the first two
stop syncing (their mail stays readable), and things made on Pro keep working
where it makes sense: scheduled messages still go out, snoozed mail still comes
back, saved templates and signatures can still be used. Rules and calendar
sync stop until Pro is back.

The code is in `apps/desktop/src-tauri/src/plan.rs` (checks and gates) and
`apps/desktop/src/lib/plan/PlanSettings.svelte`, `UpgradeDialog.svelte` and
`pro.svelte.ts` (Settings > Plan and the upgrade prompt).

## Selling Pro with Lemon Squeezy

[Lemon Squeezy](https://www.lemonsqueezy.com) is the merchant of record: it
takes the payment, charges and pays VAT, sends receipts and emails the license
key. The app talks only to its public license API, so there is no server of
our own.

1. Create a store, then a product "Novero Mail Pro" with a **single payment**
   price.
2. In the product, turn on **Generate license keys**. Set the activation limit
   to 3 (the app says one key works on three computers) and the license length
   to **unlimited**: the year of updates is checked by the app against the
   date the key was bought.
3. Note the store id (Settings > Stores) and the product id (the number in
   the product's URL), and copy the product's **checkout link** (Share).
4. In the GitHub repository, under Settings > Secrets and variables >
   Actions > **Variables**, add:
   - `NOVERO_PLANS_STORE_ID`, the store id
   - `NOVERO_PLANS_PRODUCT_ID`, the product id
   - `NOVERO_PLANS_CHECKOUT_URL`, the checkout link

   These are not secret; they end up inside the app.

Builds without `NOVERO_PLANS_STORE_ID`, such as `npm run tauri dev` without it
set, have every feature unlocked, since Pro can't be bought from them. To try
the plans locally, set the three variables before building. Lemon Squeezy's
test mode gives keys that work the same way.

## How a key is checked

- **Activate:** the key is sent to `api.lemonsqueezy.com/v1/licenses/activate`
  with the computer's name. The answer must name our store and product, so
  keys from other Lemon Squeezy shops are refused.
- **Validate:** about once a day. A refunded or disabled key turns Pro off. If
  the store can't be reached, Pro keeps working for 14 days after the last
  good check.
- **Deactivate:** Settings > Plan > "Remove key from this computer" frees the
  activation for another computer.

Everything is kept in the app's own database (the `plan` setting). A license
check is the only thing that leaves the computer, and it sends just the key
and the activation id.

## The year of updates

Each build records when it was made (`NOVERO_RELEASED`, from
`SOURCE_DATE_EPOCH` or the build time; see `src-tauri/build.rs`). A key covers
builds made up to a year after it was bought. When an update is newer than
that, its "What's new" dialog says that installing it turns Pro off.
