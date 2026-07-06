//! Zero-dependency FAQ layer: answers the common questions customers ask on
//! WhatsApp without needing any AI key. Also a light birthday detector so the
//! bot can remember a customer's birthday when they mention it.

/// Matches an inbound message against known FAQ topics. Returns a ready answer,
/// or None when nothing matches (caller then falls through to the AI/menu).
pub fn match_faq(text: &str) -> Option<String> {
    let t = text.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));

    // Order matters: most specific topics first.
    if has(&["eggless", "egg less", "without egg", "no egg"]) {
        return Some("Yes! 🥚🚫 Almost everything on our menu is available *eggless* on request — just say so when you order, or tick 'Make it eggless' on the website. We're a 100% vegetarian kitchen.".into());
    }
    if has(&[
        "allerg",
        "nut",
        "gluten",
        "dairy",
        "healthy",
        "bromate",
        "preservative",
        "additive",
        "maida",
        "vanaspati",
    ]) {
        return Some("Everything is made fresh with real butter, couverture chocolate and all-natural cocoa butter — *no potassium bromate, no artificial colours, no trans fat, no vanaspati*. 100% vegetarian. Our kitchen does handle nuts, dairy, gluten and soy, so please tell us about any serious allergy before ordering. 💛".into());
    }
    if has(&[
        "custom",
        "personalis",
        "personaliz",
        "message on",
        "name on",
        "write on",
        "design",
        "photo cake",
        "theme",
    ]) {
        return Some("We love custom cakes! 🎂 Tell us the occasion, flavour, weight, and any message you'd like written on top — reply here or tap *Browse menu* to start, and add your message at checkout.".into());
    }
    if has(&[
        "deliver",
        "delivery area",
        "where do you",
        "come to",
        "shipping",
        "outside hubli",
    ]) {
        return Some("We deliver freshly across *Hubli*, between *2pm and 10pm*, in the slot you choose. (We don't ship outside the city — everything is baked fresh for same-day enjoyment.)".into());
    }
    if has(&["time", "hour", "open", "close", "tuesday", "when are you"]) {
        return Some("We're open *2pm – 10pm every day, closed on Tuesdays*. Deliveries run in that same window.".into());
    }
    if has(&["where", "location", "address", "shop", "store"]) {
        return Some("You'll find us at *Akshay Colony, Vidya Nagar, Hubli* (opposite IBMR College). Order here on WhatsApp and we'll bring it to you. 📍".into());
    }
    if has(&[
        "pay", "payment", "upi", "card", "cash", "razorpay", "gpay", "online",
    ]) {
        return Some("You can pay securely online — after checkout we send a Razorpay link (UPI, cards, net-banking, wallets). Your order is confirmed the moment payment lands. 💳".into());
    }
    if has(&["price", "cost", "how much", "rate", "charge"]) {
        return Some("Single-serve desserts start around *₹320*; signature cakes are roughly *₹850–₹1,300*. Tap *Browse menu* to see exact prices, or tell me what you're after and I'll help. 🍰".into());
    }
    if has(&[
        "how long",
        "lead time",
        "advance",
        "same day",
        "today",
        "tomorrow",
        "notice",
    ]) {
        return Some("Most cakes can be ready *same day* within our delivery window; for larger custom cakes, a day's notice helps us make it perfect. When would you like it delivered?".into());
    }
    if has(&["veg", "vegetarian", "non veg", "egg in"]) {
        return Some("We're a *100% vegetarian* patisserie — always. Eggless options are available on request too. 🌿".into());
    }
    None
}

/// Best-effort birthday detector for messages like "my birthday is 12 August"
/// or "born on 12/08". Year is a placeholder (we only ever use month + day).
pub fn detect_birthday(text: &str) -> Option<chrono::NaiveDate> {
    let t = text.to_lowercase();
    if !(t.contains("birthday") || t.contains("bday") || t.contains("b'day") || t.contains("born"))
    {
        return None;
    }
    const MONTHS: [(&str, u32); 12] = [
        ("jan", 1),
        ("feb", 2),
        ("mar", 3),
        ("apr", 4),
        ("may", 5),
        ("jun", 6),
        ("jul", 7),
        ("aug", 8),
        ("sep", 9),
        ("oct", 10),
        ("nov", 11),
        ("dec", 12),
    ];
    let month = MONTHS.iter().find(|(m, _)| t.contains(m)).map(|(_, n)| *n);
    let nums: Vec<u32> = t
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|s| s.parse::<u32>().ok())
        .collect();

    if let Some(m) = month
        && let Some(&day) = nums.iter().find(|&&d| (1..=31).contains(&d))
    {
        return chrono::NaiveDate::from_ymd_opt(2000, m, day);
    }
    // "12/08" or "12-08" → day/month
    if month.is_none() && nums.len() >= 2 {
        let (d, m) = (nums[0], nums[1]);
        if (1..=31).contains(&d) && (1..=12).contains(&m) {
            return chrono::NaiveDate::from_ymd_opt(2000, m, d);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faq_matches_common_questions() {
        assert!(
            match_faq("do you have eggless cakes?")
                .unwrap()
                .contains("eggless")
        );
        assert!(match_faq("what time do you open").unwrap().contains("2pm"));
        assert!(
            match_faq("where are you located")
                .unwrap()
                .contains("Hubli")
        );
        assert!(match_faq("can I pay by upi").unwrap().contains("Razorpay"));
        assert!(match_faq("random gibberish xyz").is_none());
    }

    #[test]
    fn detects_birthdays() {
        assert_eq!(
            detect_birthday("my birthday is 12 August"),
            chrono::NaiveDate::from_ymd_opt(2000, 8, 12)
        );
        assert_eq!(
            detect_birthday("bday 05/09 please remember"),
            chrono::NaiveDate::from_ymd_opt(2000, 9, 5)
        );
        assert!(detect_birthday("I want a chocolate cake").is_none());
    }
}
