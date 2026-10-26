//! Sorting mail into categories (personal, newsletters, spam and so on) from
//! what a message shows about itself: its sender, its subject and the list
//! headers bulk mail carries. It all runs on the computer; nothing about the
//! mail is sent anywhere.

use serde::{Deserialize, Serialize};

/// What kind of mail a message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    Personal,
    Updates,
    Social,
    Forums,
    Newsletters,
    Promotions,
    Spam,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::Personal,
        Category::Updates,
        Category::Social,
        Category::Forums,
        Category::Newsletters,
        Category::Promotions,
        Category::Spam,
    ];

    /// Name stored in the database.
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Personal => "personal",
            Category::Updates => "updates",
            Category::Social => "social",
            Category::Forums => "forums",
            Category::Newsletters => "newsletters",
            Category::Promotions => "promotions",
            Category::Spam => "spam",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Category::ALL.into_iter().find(|c| c.as_str() == value)
    }
}

/// Hints from a message's headers, kept as bits in `messages.signals`.
pub mod signals {
    /// List-Unsubscribe: mail sent to a list the user can leave.
    pub const UNSUBSCRIBE: u32 = 1;
    /// List-Unsubscribe-Post: leaving takes one click (RFC 8058).
    pub const ONE_CLICK: u32 = 1 << 1;
    /// List-Id: a mailing list.
    pub const LIST_ID: u32 = 1 << 2;
    /// Precedence: bulk, junk or list.
    pub const BULK: u32 = 1 << 3;
    /// Auto-Submitted with any value but "no": sent by a program.
    pub const AUTOMATED: u32 = 1 << 4;
    /// X-Spam-Flag: YES or X-Spam-Status: Yes, set by the server's filter.
    pub const SPAM_FLAG: u32 = 1 << 5;
    /// Headers a bulk mailing service adds (Feedback-ID, X-Campaign...).
    pub const CAMPAIGN: u32 = 1 << 6;
}

/// Why a message got its category, shown next to the suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Reason {
    /// The server's spam filter marked it.
    SpamFilter,
    /// Its subject reads like spam.
    SpamWords,
    /// The sender's name shows a different address than the one it is from.
    NameMismatch,
    /// The user has written to the sender.
    Contact,
    /// The user replied to it.
    Replied,
    /// It is from one of the user's own addresses.
    OwnAddress,
    /// It looks like it is from a person: no list or bulk headers.
    PersonLike,
    /// From a social network.
    SocialSite,
    /// From a discussion group or mailing list service.
    Discussion,
    /// A mailing list with no unsubscribe link: a group, not marketing.
    MailingList,
    /// From a service that mostly sends notifications.
    ServiceSite,
    /// Sent by a program (Auto-Submitted).
    Automated,
    /// From a no-reply or notifications address.
    NoReply,
    /// Its subject is about an order, account, payment or the like.
    UpdateWords,
    /// Its subject is about a sale or offer.
    PromoWords,
    /// From a deals or marketing address.
    PromoSender,
    /// From a newsletter address or newsletter service.
    NewsletterSender,
    /// Its subject reads like a newsletter issue.
    NewsWords,
    /// It has an unsubscribe link.
    Unsubscribe,
    /// Sent in bulk, through a mailing service.
    BulkMail,
}

/// What the guess looks at in a message.
#[derive(Debug, Clone, Copy, Default)]
pub struct CategoryInput<'a> {
    /// The sender's address, lower case.
    pub email: &'a str,
    /// The sender's display name, if any.
    pub name: Option<&'a str>,
    pub subject: Option<&'a str>,
    /// Header hints; `None` when the headers haven't been looked at yet.
    pub signals: Option<u32>,
    /// The user has written to the sender.
    pub contact: bool,
    /// The sender is one of the user's own accounts.
    pub own: bool,
    /// The user replied to this message.
    pub answered: bool,
}

/// A suggested category, with the reasons for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Guess {
    pub category: Category,
    pub reasons: Vec<Reason>,
    /// The evidence is strong and points one way.
    pub sure: bool,
}

/// Points each category collects, with what earned them.
#[derive(Default)]
struct Scores([(i32, Vec<Reason>); 7]);

impl Scores {
    fn add(&mut self, category: Category, points: i32, reason: Reason) {
        let slot = &mut self.0[category as usize];
        slot.0 += points;
        if points > 0 && !slot.1.contains(&reason) {
            slot.1.push(reason);
        }
    }
}

/// Guesses a message's category.
pub fn guess(input: &CategoryInput<'_>) -> Guess {
    let email = input.email.trim().to_lowercase();
    let (local, domain) = email.rsplit_once('@').unwrap_or((email.as_str(), ""));
    let subject = words(input.subject.unwrap_or(""));
    let signals = input.signals.unwrap_or(0);
    let has = |bit: u32| signals & bit != 0;
    let bulk = has(signals::UNSUBSCRIBE)
        || has(signals::BULK)
        || has(signals::CAMPAIGN)
        || has(signals::AUTOMATED);
    let mut s = Scores::default();

    // Spam.
    if has(signals::SPAM_FLAG) {
        s.add(Category::Spam, 12, Reason::SpamFilter);
    }
    if phrase_in(&subject, SPAM_WORDS) {
        s.add(Category::Spam, 4, Reason::SpamWords);
    }
    if let Some(name) = input.name
        && let Some((_, shown)) = name.trim().to_lowercase().rsplit_once('@')
    {
        let shown = shown.trim_end_matches(|c: char| !c.is_alphanumeric());
        if !shown.is_empty() && !domain.is_empty() && !under(domain, shown) {
            s.add(Category::Spam, 4, Reason::NameMismatch);
        }
    }

    // Personal: people the user knows, and mail that looks hand written.
    if input.own {
        s.add(Category::Personal, 8, Reason::OwnAddress);
    }
    if input.contact {
        s.add(Category::Personal, 6, Reason::Contact);
    }
    if input.answered {
        s.add(Category::Personal, 4, Reason::Replied);
    }
    let role = role_address(local);
    if bulk {
        s.add(Category::Personal, -4, Reason::BulkMail);
    } else if input.signals.is_some() && !role {
        s.add(Category::Personal, 2, Reason::PersonLike);
    }

    // Social networks and discussion groups.
    if SOCIAL_SITES.iter().any(|site| under(domain, site)) {
        s.add(Category::Social, 7, Reason::SocialSite);
    }
    if FORUM_SITES.iter().any(|site| under(domain, site)) || domain.starts_with("lists.") {
        s.add(Category::Forums, 7, Reason::Discussion);
    }
    if has(signals::LIST_ID) && !has(signals::UNSUBSCRIBE) && !has(signals::CAMPAIGN) {
        s.add(Category::Forums, 4, Reason::MailingList);
    }

    // Updates: receipts, alerts, notifications.
    if SERVICE_SITES.iter().any(|site| under(domain, site)) {
        s.add(Category::Updates, 5, Reason::ServiceSite);
    }
    if has(signals::AUTOMATED) {
        s.add(Category::Updates, 3, Reason::Automated);
    }
    if no_reply(local) {
        s.add(Category::Updates, 3, Reason::NoReply);
    }
    if part_in(local, UPDATE_SENDERS) {
        s.add(Category::Updates, 2, Reason::NoReply);
    }
    if phrase_in(&subject, UPDATE_WORDS) {
        s.add(Category::Updates, 3, Reason::UpdateWords);
    }

    // Promotions.
    if phrase_in(&subject, PROMO_WORDS) {
        s.add(Category::Promotions, 4, Reason::PromoWords);
    }
    if part_in(local, PROMO_SENDERS) {
        s.add(Category::Promotions, 3, Reason::PromoSender);
    }

    // Newsletters, and the bulk mail nothing else claimed.
    if part_in(local, NEWS_SENDERS) || NEWS_SITES.iter().any(|site| under(domain, site)) {
        s.add(Category::Newsletters, 5, Reason::NewsletterSender);
    }
    if phrase_in(&subject, NEWS_WORDS) {
        s.add(Category::Newsletters, 2, Reason::NewsWords);
    }
    if has(signals::UNSUBSCRIBE) {
        s.add(Category::Newsletters, 2, Reason::Unsubscribe);
        s.add(Category::Promotions, 1, Reason::Unsubscribe);
    }
    if has(signals::BULK) || has(signals::CAMPAIGN) {
        s.add(Category::Newsletters, 1, Reason::BulkMail);
        s.add(Category::Promotions, 1, Reason::BulkMail);
    }

    // The highest score wins; on a tie, the earlier category in this order.
    const ORDER: [Category; 7] = [
        Category::Spam,
        Category::Personal,
        Category::Social,
        Category::Forums,
        Category::Updates,
        Category::Promotions,
        Category::Newsletters,
    ];
    let mut ranked: Vec<(i32, Category)> = ORDER.iter().map(|&c| (s.0[c as usize].0, c)).collect();
    ranked.sort_by_key(|&(points, _)| std::cmp::Reverse(points));
    let (best, category) = ranked[0];
    let second = ranked[1].0;
    if best <= 0 {
        // Nothing to go on: most likely a person writing.
        return Guess {
            category: Category::Personal,
            reasons: Vec::new(),
            sure: false,
        };
    }
    Guess {
        category,
        reasons: std::mem::take(&mut s.0[category as usize].1),
        sure: best >= 5 && best - second.max(0) >= 3,
    }
}

/// The subject in lower case with punctuation turned into spaces and a
/// space at each end, so phrases match whole words.
fn words(subject: &str) -> String {
    let mut out = String::with_capacity(subject.len() + 2);
    out.push(' ');
    for c in subject.chars().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '%' {
            out.push(c);
        } else if c == '\'' || c == '’' {
            // "don't" stays one word.
        } else if !out.ends_with(' ') {
            out.push(' ');
        }
    }
    // "50%off" and "50% off" both read as "% off".
    let mut out = out.replace('%', " % ");
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    if !out.ends_with(' ') {
        out.push(' ');
    }
    out
}

fn phrase_in(subject: &str, phrases: &[&str]) -> bool {
    phrases.iter().any(|p| subject.contains(&format!(" {p} ")))
}

/// Whether `domain` is `site` or one of its subdomains.
fn under(domain: &str, site: &str) -> bool {
    domain == site
        || domain
            .strip_suffix(site)
            .is_some_and(|rest| rest.ends_with('.'))
}

/// The parts of an address's local part: "deals.news-uk" is deals, news, uk.
fn parts(local: &str) -> impl Iterator<Item = &str> {
    local
        .split(['.', '-', '_', '+'])
        .map(|p| p.trim_end_matches(|c: char| c.is_ascii_digit()))
        .filter(|p| !p.is_empty())
}

fn part_in(local: &str, list: &[&str]) -> bool {
    parts(local).any(|p| list.contains(&p))
}

fn no_reply(local: &str) -> bool {
    let joined: String = local.chars().filter(char::is_ascii_alphanumeric).collect();
    NO_REPLY.iter().any(|n| joined.starts_with(n))
        || part_in(local, NO_REPLY)
        || part_in(local, NOTIFY_SENDERS)
}

/// An address that belongs to a role or system rather than a person.
fn role_address(local: &str) -> bool {
    no_reply(local)
        || part_in(local, UPDATE_SENDERS)
        || part_in(local, PROMO_SENDERS)
        || part_in(local, NEWS_SENDERS)
        || part_in(local, OTHER_ROLES)
}

const SPAM_WORDS: &[&str] = &[
    "you have won",
    "you ve won",
    "youve won",
    "winner",
    "lottery",
    "claim your prize",
    "claim your reward",
    "inheritance",
    "beneficiary",
    "wire transfer",
    "million dollars",
    "million usd",
    "crypto investment",
    "bitcoin investment",
    "double your",
    "viagra",
    "cialis",
    "casino",
    "weight loss",
    "lose weight",
    "hot singles",
    "work from home",
    "make money fast",
    "earn money",
    "risk free",
    "100 % free",
    "act now",
    "urgent reply",
    "urgent response",
    "loan approved",
    "pre approved",
    "account will be suspended",
    "account has been suspended",
    "final notice",
];

const UPDATE_WORDS: &[&str] = &[
    "receipt",
    "invoice",
    "your order",
    "order confirmation",
    "order confirmed",
    "has shipped",
    "shipped",
    "out for delivery",
    "delivered",
    "delivery",
    "tracking",
    "payment",
    "payment received",
    "statement",
    "password",
    "verification code",
    "verify your",
    "security alert",
    "security code",
    "sign in",
    "new sign in",
    "login",
    "one time code",
    "confirm your",
    "confirmation",
    "booking",
    "reservation",
    "itinerary",
    "ticket",
    "reminder",
    "your account",
    "subscription",
    "renewal",
    "refund",
    "appointment",
    "notification",
];

const PROMO_WORDS: &[&str] = &[
    "sale",
    "deal",
    "deals",
    "discount",
    "offer",
    "offers",
    "coupon",
    "promo",
    "promo code",
    "free shipping",
    "limited time",
    "last chance",
    "black friday",
    "cyber monday",
    "shop now",
    "new arrivals",
    "exclusive",
    "ends tonight",
    "ends today",
    "today only",
    "clearance",
    "flash sale",
    "gift guide",
    "dont miss",
    "save up to",
    "% off",
];

const NEWS_WORDS: &[&str] = &[
    "newsletter",
    "digest",
    "weekly",
    "this week",
    "edition",
    "roundup",
    "briefing",
    "daily",
    "monthly update",
];

const NO_REPLY: &[&str] = &["noreply", "donotreply", "mailerdaemon", "postmaster"];

const NOTIFY_SENDERS: &[&str] = &[
    "notification",
    "notifications",
    "notify",
    "alert",
    "alerts",
    "automated",
    "bounce",
    "bounces",
];

const UPDATE_SENDERS: &[&str] = &[
    "account",
    "accounts",
    "security",
    "billing",
    "receipt",
    "receipts",
    "order",
    "orders",
    "shipping",
    "invoice",
    "invoices",
    "payment",
    "payments",
    "calendar",
    "reminder",
    "reminders",
    "verify",
];

const PROMO_SENDERS: &[&str] = &[
    "deals",
    "offers",
    "offer",
    "promo",
    "promos",
    "promotions",
    "promotion",
    "marketing",
    "sales",
    "shop",
    "store",
    "specials",
];

const NEWS_SENDERS: &[&str] = &[
    "newsletter",
    "newsletters",
    "news",
    "digest",
    "weekly",
    "editor",
    "editors",
    "briefing",
];

const OTHER_ROLES: &[&str] = &[
    "info", "hello", "team", "support", "help", "contact", "service", "admin", "mail", "email",
];

const SOCIAL_SITES: &[&str] = &[
    "facebookmail.com",
    "facebook.com",
    "linkedin.com",
    "twitter.com",
    "x.com",
    "instagram.com",
    "tiktok.com",
    "pinterest.com",
    "reddit.com",
    "redditmail.com",
    "discord.com",
    "snapchat.com",
    "youtube.com",
    "nextdoor.com",
    "meetup.com",
    "quora.com",
    "tumblr.com",
    "threads.net",
    "bsky.app",
    "strava.com",
    "twitch.tv",
];

const FORUM_SITES: &[&str] = &[
    "googlegroups.com",
    "groups.io",
    "discoursemail.com",
    "freelists.org",
    "lists.sourceforge.net",
];

const SERVICE_SITES: &[&str] = &[
    "github.com",
    "gitlab.com",
    "atlassian.net",
    "bitbucket.org",
    "slack.com",
    "notion.so",
    "trello.com",
    "asana.com",
    "calendly.com",
    "docusign.net",
];

const NEWS_SITES: &[&str] = &[
    "substack.com",
    "beehiiv.com",
    "buttondown.email",
    "ghost.io",
    "convertkit-mail.com",
    "convertkit-mail2.com",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn input<'a>(email: &'a str, subject: &'a str, signals: u32) -> CategoryInput<'a> {
        CategoryInput {
            email,
            subject: Some(subject),
            signals: Some(signals),
            ..CategoryInput::default()
        }
    }

    fn category(i: CategoryInput<'_>) -> Category {
        guess(&i).category
    }

    #[test]
    fn the_spam_filter_wins_over_everything() {
        let mut i = input("friend@home.test", "Lunch?", signals::SPAM_FLAG);
        i.contact = true;
        let g = guess(&i);
        assert_eq!(g.category, Category::Spam);
        assert!(g.reasons.contains(&Reason::SpamFilter));
        assert_eq!(
            category(input("x@lucky.test", "You have WON the lottery!!", 0)),
            Category::Spam
        );
    }

    #[test]
    fn a_name_showing_another_address_looks_like_spam() {
        let i = CategoryInput {
            name: Some("support@paypal.com"),
            ..input("random@cheap-host.test", "Your account", 0)
        };
        assert_eq!(category(i), Category::Spam);
        let fine = CategoryInput {
            name: Some("ann@shop.test"),
            ..input("ann@mail.shop.test", "Hi", 0)
        };
        assert_ne!(category(fine), Category::Spam);
    }

    #[test]
    fn people_the_user_writes_to_are_personal() {
        let mut i = input("ann@home.test", "Photos from Saturday", 0);
        assert_eq!(category(i), Category::Personal);
        i.contact = true;
        let g = guess(&i);
        assert_eq!(g.category, Category::Personal);
        assert!(g.sure);
        assert!(g.reasons.contains(&Reason::Contact));
    }

    #[test]
    fn bulk_mail_is_sorted_by_sender_and_subject() {
        let list = signals::UNSUBSCRIBE | signals::CAMPAIGN;
        assert_eq!(
            category(input(
                "deals@shop.test",
                "Black Friday: 40% off everything",
                list
            )),
            Category::Promotions
        );
        assert_eq!(
            category(input("hello@shop.test", "Save 50%OFF today only", list)),
            Category::Promotions
        );
        assert_eq!(
            category(input("newsletter@paper.test", "The weekly digest", list)),
            Category::Newsletters
        );
        assert_eq!(
            category(input("ann@substack.com", "Thoughts on gardens", list)),
            Category::Newsletters
        );
        assert_eq!(
            category(input("no-reply@shop.test", "Your order has shipped", list)),
            Category::Updates
        );
        assert_eq!(
            category(input("notifications@github.com", "[repo] New issue", list)),
            Category::Updates
        );
        assert_eq!(
            category(input("hello@studio.test", "Something new from us", list)),
            Category::Newsletters
        );
    }

    #[test]
    fn social_sites_and_groups_have_their_own_categories() {
        assert_eq!(
            category(input(
                "messages-noreply@linkedin.com",
                "You appeared in 4 searches",
                signals::UNSUBSCRIBE
            )),
            Category::Social
        );
        assert_eq!(
            category(input("rust-users@googlegroups.com", "Re: lifetimes", 0)),
            Category::Forums
        );
        assert_eq!(
            category(input(
                "dev@project.test",
                "Re: release plan",
                signals::LIST_ID | signals::BULK
            )),
            Category::Forums
        );
    }

    #[test]
    fn unknown_headers_lean_personal_but_unsure() {
        let i = CategoryInput {
            signals: None,
            ..input("ann@home.test", "Hello", 0)
        };
        let g = guess(&i);
        assert_eq!(g.category, Category::Personal);
        assert!(!g.sure);
    }

    #[test]
    fn subjects_match_whole_words() {
        assert!(phrase_in(&words("Sales report Q3"), &["sales report q3"]));
        assert!(!phrase_in(&words("Wholesale prices"), &["sale"]));
        assert!(phrase_in(&words("Don't miss this!"), &["dont miss"]));
        assert!(phrase_in(&words("Now 50%off"), &["% off"]));
    }

    #[test]
    fn domains_match_themselves_and_subdomains_only() {
        assert!(under("linkedin.com", "linkedin.com"));
        assert!(under("e.linkedin.com", "linkedin.com"));
        assert!(!under("notlinkedin.com", "linkedin.com"));
    }

    #[test]
    fn categories_round_trip_through_their_names() {
        for c in Category::ALL {
            assert_eq!(Category::parse(c.as_str()), Some(c));
        }
        assert_eq!(Category::parse("other"), None);
    }
}
