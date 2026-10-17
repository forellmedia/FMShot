//! Feste Begriffsliste für Stichwörter und Schnellfilter (ADR 0010).
//!
//! Jeder Begriff hat einen Schlüssel (für Übersetzung `tag.<key>` und Index),
//! eine Gruppe (Schnellfilter `group.<group>`) und eine englische
//! Beschreibung, mit der das Modell ihn erkennt – englisch, weil SigLIP damit
//! am treffsichersten ist. Angezeigt wird die Übersetzung.

/// Gruppen in der Reihenfolge der Schnellfilter. `objects` hat keinen Filter.
pub const GROUPS: [&str; 10] = [
    "people",
    "animals",
    "nature",
    "food",
    "vehicles",
    "buildings",
    "documents",
    "code",
    "charts",
    "apps",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Term {
    pub key: &'static str,
    pub group: &'static str,
    pub prompt: &'static str,
}

const fn t(key: &'static str, group: &'static str, prompt: &'static str) -> Term {
    Term { key, group, prompt }
}

pub const TERMS: &[Term] = &[
    t("person", "people", "a photo of a person"),
    t("man", "people", "a photo of a man"),
    t("woman", "people", "a photo of a woman"),
    t("child", "people", "a photo of a child"),
    t("baby", "people", "a photo of a baby"),
    t("group", "people", "a photo of a group of people"),
    t("face", "people", "a close-up photo of a face"),
    t("selfie", "people", "a selfie"),
    t("crowd", "people", "a photo of a crowd"),
    t("family", "people", "a photo of a family"),
    t("dog", "animals", "a photo of a dog"),
    t("cat", "animals", "a photo of a cat"),
    t("bird", "animals", "a photo of a bird"),
    t("horse", "animals", "a photo of a horse"),
    t("cow", "animals", "a photo of a cow"),
    t("fish", "animals", "a photo of a fish"),
    t("insect", "animals", "a photo of an insect"),
    t("butterfly", "animals", "a photo of a butterfly"),
    t("sheep", "animals", "a photo of a sheep"),
    t("pig", "animals", "a photo of a pig"),
    t("rabbit", "animals", "a photo of a rabbit"),
    t("wild_animal", "animals", "a photo of a wild animal"),
    t("forest", "nature", "a photo of a forest"),
    t("tree", "nature", "a photo of a tree"),
    t("mountain", "nature", "a photo of mountains"),
    t("lake", "nature", "a photo of a lake"),
    t("river", "nature", "a photo of a river"),
    t("sea", "nature", "a photo of the sea"),
    t("beach", "nature", "a photo of a beach"),
    t("sky", "nature", "a photo of the sky with clouds"),
    t("sunset", "nature", "a photo of a sunset"),
    t("snow", "nature", "a photo of snow"),
    t("flower", "nature", "a photo of flowers"),
    t("garden", "nature", "a photo of a garden"),
    t("meadow", "nature", "a photo of a field or meadow"),
    t("landscape", "nature", "a landscape photo"),
    t("desert", "nature", "a photo of a desert"),
    t("night", "nature", "a photo taken at night"),
    t("food", "food", "a photo of food"),
    t("pizza", "food", "a photo of a pizza"),
    t("cake", "food", "a photo of a cake"),
    t("fruit", "food", "a photo of fruit"),
    t("vegetables", "food", "a photo of vegetables"),
    t("bread", "food", "a photo of bread"),
    t("coffee", "food", "a photo of a cup of coffee"),
    t("drink", "food", "a photo of a drink"),
    t("wine", "food", "a photo of wine"),
    t("restaurant", "food", "a photo of a restaurant"),
    t("car", "vehicles", "a photo of a car"),
    t("bicycle", "vehicles", "a photo of a bicycle"),
    t("motorcycle", "vehicles", "a photo of a motorcycle"),
    t("bus", "vehicles", "a photo of a bus"),
    t("train", "vehicles", "a photo of a train"),
    t("airplane", "vehicles", "a photo of an airplane"),
    t("boat", "vehicles", "a photo of a boat"),
    t("truck", "vehicles", "a photo of a truck"),
    t("house", "buildings", "a photo of a house"),
    t("building", "buildings", "a photo of a building"),
    t("city", "buildings", "a photo of a city"),
    t("street", "buildings", "a photo of a street"),
    t("bridge", "buildings", "a photo of a bridge"),
    t("church", "buildings", "a photo of a church"),
    t("room", "buildings", "a photo of a room interior"),
    t("office", "buildings", "a photo of an office"),
    t("kitchen", "buildings", "a photo of a kitchen"),
    t("document", "documents", "a screenshot of a text document"),
    t("invoice", "documents", "a screenshot of an invoice"),
    t("letter", "documents", "a scanned letter"),
    t("form", "documents", "a screenshot of a form to fill in"),
    t("email", "documents", "a screenshot of an email"),
    t("pdf", "documents", "a screenshot of a PDF page"),
    t("book_page", "documents", "a photo of a book page"),
    t("receipt", "documents", "a photo of a receipt"),
    t("handwriting", "documents", "a photo of handwriting"),
    t("code", "code", "a screenshot of source code"),
    t("terminal", "code", "a screenshot of a terminal window"),
    t("error_message", "code", "a screenshot of an error message"),
    t("chart", "charts", "a screenshot of a chart"),
    t("bar_chart", "charts", "a bar chart"),
    t("table", "charts", "a screenshot of a table"),
    t("spreadsheet", "charts", "a screenshot of a spreadsheet"),
    t("map", "charts", "a screenshot of a map"),
    t("diagram", "charts", "a diagram with boxes and arrows"),
    t("presentation", "charts", "a presentation slide"),
    t("website", "apps", "a screenshot of a website"),
    t("app", "apps", "a screenshot of an app"),
    t("login", "apps", "a screenshot of a login page"),
    t("chat", "apps", "a screenshot of a chat conversation"),
    t(
        "social_media",
        "apps",
        "a screenshot of a social media post",
    ),
    t("video_call", "apps", "a screenshot of a video call"),
    t("settings", "apps", "a screenshot of a settings window"),
    t("calendar", "apps", "a screenshot of a calendar"),
    t("shop", "apps", "a screenshot of an online shop"),
    t("video", "apps", "a screenshot of a video player"),
    t("game", "apps", "a screenshot of a video game"),
    t("desktop", "apps", "a screenshot of a computer desktop"),
    t("logo", "objects", "a logo"),
    t("icon", "objects", "an icon"),
    t("drawing", "objects", "a drawing or illustration"),
    t("painting", "objects", "a painting"),
    t("computer", "objects", "a photo of a computer"),
    t("keyboard", "objects", "a photo of a keyboard"),
    t("phone", "objects", "a photo of a smartphone"),
    t("book", "objects", "a photo of a book"),
    t("ball", "objects", "a photo of a ball"),
    t("sport", "objects", "a photo of people doing sports"),
    t("music", "objects", "a photo of a musical instrument"),
    t("clothes", "objects", "a photo of clothes"),
    t("money", "objects", "a photo of money"),
    t("furniture", "objects", "a photo of furniture"),
    t("toy", "objects", "a photo of a toy"),
];

/// Füllwörter ohne Bildinhalt. Ihr Mittel ist der Grundwert eines Bildes:
/// Bildschirmfotos mit viel Text passen sonst zu fast jedem Suchwort.
pub const NEUTRAL: [&str; 8] = [
    "das",
    "und",
    "ist",
    "etwas",
    "ein",
    "the",
    "and",
    "something",
];

/// Begriffe einer Gruppe.
pub fn in_group(group: &str) -> impl Iterator<Item = &'static Term> + '_ {
    TERMS.iter().filter(move |t| t.group == group)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schluessel_eindeutig_und_gruppen_bekannt() {
        let mut keys: Vec<_> = TERMS.iter().map(|t| t.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), TERMS.len());
        for term in TERMS {
            assert!(
                term.group == "objects" || GROUPS.contains(&term.group),
                "{}",
                term.key
            );
        }
        for g in GROUPS {
            assert!(in_group(g).next().is_some(), "{g}");
        }
    }
}
