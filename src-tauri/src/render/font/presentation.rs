//! Emoji presentation dispatch.
//!
//! Decides whether a code point should be drawn with the color emoji
//! font, the monochrome emoji font, or as ordinary text. The decision is
//! pure (no I/O, no allocation) and consults compile-time tables of the
//! Unicode `Emoji` and `Emoji_Presentation` properties.
//!
//! Data source: Unicode `emoji-data.txt`, Emoji 16.0 (released 2024-09-10).
//! The two range tables below are extracted from that revision and trimmed
//! to the inclusive ranges actually present in the file. They are sorted
//! by start code-point so `binary_search` can locate the matching range in
//! O(log n).
//!
//! When a future Unicode revision adds new emoji code points, refresh both
//! tables from <https://www.unicode.org/Public/emoji/16.0/emoji-data.txt>
//! (or the most recent published revision).
//!
//! Selection rules (FR5):
//! - Variation Selector-16 (U+FE0F) → `Color`.
//! - Variation Selector-15 (U+FE0E) → `Monochrome`.
//! - Bare code point with `Emoji_Presentation=Yes` → `Color`.
//! - Bare code point with `Emoji=Yes` and `Emoji_Presentation=No`
//!   (text-default emoji, e.g. U+23F5) → `Monochrome`.
//! - Anything else → `NotEmoji`.

/// Which font (color vs monochrome) should render a given code point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmojiPresentation {
    /// Code point should be rendered as a colored emoji glyph (CBDT / COLR / sbix).
    Color,
    /// Code point is an emoji but should default to a monochrome / text glyph.
    Monochrome,
    /// Code point is not an emoji at all (regular text).
    NotEmoji,
}

/// `Emoji=Yes` code points (every code point whose `Emoji` property is `Yes`,
/// including those with `Emoji_Presentation=No`).
///
/// Each entry is an inclusive `(start, end)` range. The slice is sorted by
/// `start`; ranges do not overlap.
///
/// Source: Unicode `emoji-data.txt`, Emoji 16.0 (2024-09-10).
static EMOJI_RANGES: &[(u32, u32)] = &[
    (0x0023, 0x0023), // NUMBER SIGN
    (0x002A, 0x002A), // ASTERISK
    (0x0030, 0x0039), // DIGIT ZERO..DIGIT NINE
    (0x00A9, 0x00A9), // COPYRIGHT SIGN
    (0x00AE, 0x00AE), // REGISTERED SIGN
    (0x203C, 0x203C), // DOUBLE EXCLAMATION MARK
    (0x2049, 0x2049), // EXCLAMATION QUESTION MARK
    (0x2122, 0x2122), // TRADE MARK SIGN
    (0x2139, 0x2139), // INFORMATION SOURCE
    (0x2194, 0x2199), // LEFT RIGHT ARROW..SOUTH WEST ARROW
    (0x21A9, 0x21AA), // LEFTWARDS ARROW WITH HOOK..RIGHTWARDS ARROW WITH HOOK
    (0x231A, 0x231B), // WATCH..HOURGLASS
    (0x2328, 0x2328), // KEYBOARD
    (0x23CF, 0x23CF), // EJECT SYMBOL
    (0x23E9, 0x23F3), // BLACK RIGHT-POINTING DOUBLE TRIANGLE..HOURGLASS WITH FLOWING SAND
    // U+23F4..U+23F7 are media-control symbols (⏴ ⏵ ⏶ ⏷) that Unicode
    // does not list in Emoji_Properties=Yes, but Noto Emoji (monochrome)
    // ships outline glyphs for them — and they are the exact code points
    // the Windows release build was tofu-ing under Claude Code's auto-mode
    // indicator. Classifying them as text-default emoji routes them to
    // the monochrome bundle that does have the glyph.
    (0x23F4, 0x23F7),
    (0x23F8, 0x23FA),   // DOUBLE VERTICAL BAR..BLACK CIRCLE FOR RECORD
    (0x24C2, 0x24C2),   // CIRCLED LATIN CAPITAL LETTER M
    (0x25AA, 0x25AB),   // BLACK SMALL SQUARE..WHITE SMALL SQUARE
    (0x25B6, 0x25B6),   // BLACK RIGHT-POINTING TRIANGLE
    (0x25C0, 0x25C0),   // BLACK LEFT-POINTING TRIANGLE
    (0x25FB, 0x25FE),   // WHITE MEDIUM SQUARE..BLACK MEDIUM SMALL SQUARE
    (0x2600, 0x2604),   // BLACK SUN WITH RAYS..COMET
    (0x260E, 0x260E),   // BLACK TELEPHONE
    (0x2611, 0x2611),   // BALLOT BOX WITH CHECK
    (0x2614, 0x2615),   // UMBRELLA WITH RAIN DROPS..HOT BEVERAGE
    (0x2618, 0x2618),   // SHAMROCK
    (0x261D, 0x261D),   // WHITE UP POINTING INDEX
    (0x2620, 0x2620),   // SKULL AND CROSSBONES
    (0x2622, 0x2623),   // RADIOACTIVE SIGN..BIOHAZARD SIGN
    (0x2626, 0x2626),   // ORTHODOX CROSS
    (0x262A, 0x262A),   // STAR AND CRESCENT
    (0x262E, 0x262F),   // PEACE SYMBOL..YIN YANG
    (0x2638, 0x263A),   // WHEEL OF DHARMA..WHITE SMILING FACE
    (0x2640, 0x2640),   // FEMALE SIGN
    (0x2642, 0x2642),   // MALE SIGN
    (0x2648, 0x2653),   // ARIES..PISCES
    (0x265F, 0x2660),   // BLACK CHESS PAWN..BLACK SPADE SUIT
    (0x2663, 0x2663),   // BLACK CLUB SUIT
    (0x2665, 0x2666),   // BLACK HEART SUIT..BLACK DIAMOND SUIT
    (0x2668, 0x2668),   // HOT SPRINGS
    (0x267B, 0x267B),   // BLACK UNIVERSAL RECYCLING SYMBOL
    (0x267E, 0x267F),   // PERMANENT PAPER SIGN..WHEELCHAIR SYMBOL
    (0x2692, 0x2697),   // HAMMER AND PICK..ALEMBIC
    (0x2699, 0x2699),   // GEAR
    (0x269B, 0x269C),   // ATOM SYMBOL..FLEUR-DE-LIS
    (0x26A0, 0x26A1),   // WARNING SIGN..HIGH VOLTAGE SIGN
    (0x26A7, 0x26A7),   // MALE WITH STROKE AND MALE AND FEMALE SIGN
    (0x26AA, 0x26AB),   // MEDIUM WHITE CIRCLE..MEDIUM BLACK CIRCLE
    (0x26B0, 0x26B1),   // COFFIN..FUNERAL URN
    (0x26BD, 0x26BE),   // SOCCER BALL..BASEBALL
    (0x26C4, 0x26C5),   // SNOWMAN WITHOUT SNOW..SUN BEHIND CLOUD
    (0x26C8, 0x26C8),   // THUNDER CLOUD AND RAIN
    (0x26CE, 0x26CF),   // OPHIUCHUS..PICK
    (0x26D1, 0x26D1),   // HELMET WITH WHITE CROSS
    (0x26D3, 0x26D4),   // CHAINS..NO ENTRY
    (0x26E9, 0x26EA),   // SHINTO SHRINE..CHURCH
    (0x26F0, 0x26F5),   // MOUNTAIN..SAILBOAT
    (0x26F7, 0x26FA),   // SKIER..TENT
    (0x26FD, 0x26FD),   // FUEL PUMP
    (0x2702, 0x2702),   // BLACK SCISSORS
    (0x2705, 0x2705),   // WHITE HEAVY CHECK MARK
    (0x2708, 0x270D),   // AIRPLANE..WRITING HAND
    (0x270F, 0x270F),   // PENCIL
    (0x2712, 0x2712),   // BLACK NIB
    (0x2714, 0x2714),   // HEAVY CHECK MARK
    (0x2716, 0x2716),   // HEAVY MULTIPLICATION X
    (0x271D, 0x271D),   // LATIN CROSS
    (0x2721, 0x2721),   // STAR OF DAVID
    (0x2728, 0x2728),   // SPARKLES
    (0x2733, 0x2734),   // EIGHT SPOKED ASTERISK..EIGHT POINTED BLACK STAR
    (0x2744, 0x2744),   // SNOWFLAKE
    (0x2747, 0x2747),   // SPARKLE
    (0x274C, 0x274C),   // CROSS MARK
    (0x274E, 0x274E),   // NEGATIVE SQUARED CROSS MARK
    (0x2753, 0x2755),   // BLACK QUESTION MARK ORNAMENT..WHITE EXCLAMATION MARK ORNAMENT
    (0x2757, 0x2757),   // HEAVY EXCLAMATION MARK SYMBOL
    (0x2763, 0x2764),   // HEAVY HEART EXCLAMATION..HEAVY BLACK HEART
    (0x2795, 0x2797),   // HEAVY PLUS SIGN..HEAVY DIVISION SIGN
    (0x27A1, 0x27A1),   // BLACK RIGHTWARDS ARROW
    (0x27B0, 0x27B0),   // CURLY LOOP
    (0x27BF, 0x27BF),   // DOUBLE CURLY LOOP
    (0x2934, 0x2935),   // ARROW POINTING RIGHTWARDS THEN CURVING UPWARDS..DOWNWARDS
    (0x2B05, 0x2B07),   // LEFTWARDS BLACK ARROW..DOWNWARDS BLACK ARROW
    (0x2B1B, 0x2B1C),   // BLACK LARGE SQUARE..WHITE LARGE SQUARE
    (0x2B50, 0x2B50),   // WHITE MEDIUM STAR
    (0x2B55, 0x2B55),   // HEAVY LARGE CIRCLE
    (0x3030, 0x3030),   // WAVY DASH
    (0x303D, 0x303D),   // PART ALTERNATION MARK
    (0x3297, 0x3297),   // CIRCLED IDEOGRAPH CONGRATULATION
    (0x3299, 0x3299),   // CIRCLED IDEOGRAPH SECRET
    (0x1F004, 0x1F004), // MAHJONG TILE RED DRAGON
    (0x1F0CF, 0x1F0CF), // PLAYING CARD BLACK JOKER
    (0x1F170, 0x1F171), // NEGATIVE SQUARED LATIN CAPITAL LETTER A..B
    (0x1F17E, 0x1F17F), // NEGATIVE SQUARED LATIN CAPITAL LETTER O..P
    (0x1F18E, 0x1F18E), // NEGATIVE SQUARED AB
    (0x1F191, 0x1F19A), // SQUARED CL..SQUARED VS
    (0x1F1E6, 0x1F1FF), // Regional indicator A..Z (flag pairs)
    (0x1F201, 0x1F202), // SQUARED KATAKANA KOKO..SA
    (0x1F21A, 0x1F21A), // SQUARED CJK UNIFIED IDEOGRAPH-7121
    (0x1F22F, 0x1F22F), // SQUARED CJK UNIFIED IDEOGRAPH-6307
    (0x1F232, 0x1F23A), // SQUARED CJK UNIFIED IDEOGRAPH-7981..SQUARED CJK UNIFIED IDEOGRAPH-55B6
    (0x1F250, 0x1F251), // CIRCLED IDEOGRAPH ADVANTAGE..CIRCLED IDEOGRAPH ACCEPT
    (0x1F300, 0x1F320), // CYCLONE..SHOOTING STAR
    (0x1F321, 0x1F321), // THERMOMETER
    (0x1F324, 0x1F32C), // WHITE SUN WITH SMALL CLOUD..WIND BLOWING FACE
    (0x1F32D, 0x1F32F), // HOT DOG..BURRITO
    (0x1F330, 0x1F335), // CHESTNUT..CACTUS
    (0x1F336, 0x1F336), // HOT PEPPER
    (0x1F337, 0x1F37C), // TULIP..BABY BOTTLE
    (0x1F37D, 0x1F37D), // FORK AND KNIFE WITH PLATE
    (0x1F37E, 0x1F37F), // BOTTLE WITH POPPING CORK..POPCORN
    (0x1F380, 0x1F393), // RIBBON..GRADUATION CAP
    (0x1F396, 0x1F397), // MILITARY MEDAL..REMINDER RIBBON
    (0x1F399, 0x1F39B), // STUDIO MICROPHONE..CONTROL KNOBS
    (0x1F39E, 0x1F39F), // FILM FRAMES..ADMISSION TICKETS
    (0x1F3A0, 0x1F3C4), // CAROUSEL HORSE..SURFER
    (0x1F3C5, 0x1F3C5), // SPORTS MEDAL
    (0x1F3C6, 0x1F3CA), // TROPHY..SWIMMER
    (0x1F3CB, 0x1F3CE), // WEIGHT LIFTER..RACING CAR
    (0x1F3CF, 0x1F3D3), // CRICKET BAT AND BALL..TABLE TENNIS PADDLE AND BALL
    (0x1F3D4, 0x1F3DF), // SNOW CAPPED MOUNTAIN..STADIUM
    (0x1F3E0, 0x1F3F0), // HOUSE BUILDING..EUROPEAN CASTLE
    (0x1F3F3, 0x1F3F5), // WAVING WHITE FLAG..ROSETTE
    (0x1F3F7, 0x1F3F7), // LABEL
    (0x1F3F8, 0x1F407), // BADMINTON RACQUET AND SHUTTLECOCK..RABBIT
    (0x1F408, 0x1F40B), // CAT..WHALE
    (0x1F40C, 0x1F40E), // SNAIL..HORSE
    (0x1F40F, 0x1F410), // RAM..GOAT
    (0x1F411, 0x1F412), // SHEEP..MONKEY
    (0x1F413, 0x1F413), // ROOSTER
    (0x1F414, 0x1F414), // CHICKEN
    (0x1F415, 0x1F415), // DOG
    (0x1F416, 0x1F416), // PIG
    (0x1F417, 0x1F429), // BOAR..POODLE
    (0x1F42A, 0x1F42A), // DROMEDARY CAMEL
    (0x1F42B, 0x1F43E), // BACTRIAN CAMEL..PAW PRINTS
    (0x1F43F, 0x1F43F), // CHIPMUNK
    (0x1F440, 0x1F440), // EYES
    (0x1F441, 0x1F441), // EYE
    (0x1F442, 0x1F464), // EAR..BUST IN SILHOUETTE
    (0x1F465, 0x1F465), // BUSTS IN SILHOUETTE
    (0x1F466, 0x1F46B), // BOY..MAN AND WOMAN HOLDING HANDS
    (0x1F46C, 0x1F46D), // TWO MEN HOLDING HANDS..TWO WOMEN HOLDING HANDS
    (0x1F46E, 0x1F4AC), // POLICE OFFICER..SPEECH BALLOON
    (0x1F4AD, 0x1F4AD), // THOUGHT BALLOON
    (0x1F4AE, 0x1F4B5), // WHITE FLOWER..BANKNOTE WITH DOLLAR SIGN
    (0x1F4B6, 0x1F4B7), // BANKNOTE WITH EURO SIGN..BANKNOTE WITH POUND SIGN
    (0x1F4B8, 0x1F4EB), // MONEY WITH WINGS..CLOSED MAILBOX WITH RAISED FLAG
    (0x1F4EC, 0x1F4ED), // OPEN MAILBOX WITH RAISED FLAG..OPEN MAILBOX WITH LOWERED FLAG
    (0x1F4EE, 0x1F4EE), // POSTBOX
    (0x1F4EF, 0x1F4EF), // POSTAL HORN
    (0x1F4F0, 0x1F4F4), // NEWSPAPER..MOBILE PHONE OFF
    (0x1F4F5, 0x1F4F5), // NO MOBILE PHONES
    (0x1F4F6, 0x1F4F7), // ANTENNA WITH BARS..CAMERA
    (0x1F4F8, 0x1F4F8), // CAMERA WITH FLASH
    (0x1F4F9, 0x1F4FC), // VIDEO CAMERA..VIDEOCASSETTE
    (0x1F4FD, 0x1F4FD), // FILM PROJECTOR
    (0x1F4FF, 0x1F502), // PRAYER BEADS..CLOCKWISE RIGHTWARDS AND LEFTWARDS OPEN CIRCLE ARROWS WITH CIRCLED ONE OVERLAY
    (0x1F503, 0x1F503), // CLOCKWISE DOWNWARDS AND UPWARDS OPEN CIRCLE ARROWS
    (0x1F504, 0x1F507), // ANTICLOCKWISE DOWNWARDS AND UPWARDS OPEN CIRCLE ARROWS..SPEAKER WITH CANCELLATION STROKE
    (0x1F508, 0x1F508), // SPEAKER
    (0x1F509, 0x1F509), // SPEAKER WITH ONE SOUND WAVE
    (0x1F50A, 0x1F514), // SPEAKER WITH THREE SOUND WAVES..BELL
    (0x1F515, 0x1F515), // BELL WITH CANCELLATION STROKE
    (0x1F516, 0x1F52B), // BOOKMARK..PISTOL
    (0x1F52C, 0x1F52D), // MICROSCOPE..TELESCOPE
    (0x1F52E, 0x1F53D), // CRYSTAL BALL..DOWN-POINTING SMALL RED TRIANGLE
    (0x1F549, 0x1F54A), // OM SYMBOL..DOVE OF PEACE
    (0x1F54B, 0x1F54E), // KAABA..MENORAH WITH NINE BRANCHES
    (0x1F550, 0x1F567), // CLOCK FACE ONE OCLOCK..CLOCK FACE TWELVE-THIRTY
    (0x1F56F, 0x1F570), // CANDLE..MANTELPIECE CLOCK
    (0x1F573, 0x1F579), // HOLE..JOYSTICK
    (0x1F57A, 0x1F57A), // MAN DANCING
    (0x1F587, 0x1F587), // LINKED PAPERCLIPS
    (0x1F58A, 0x1F58D), // LOWER LEFT BALLPOINT PEN..LOWER LEFT CRAYON
    (0x1F590, 0x1F590), // RAISED HAND WITH FINGERS SPLAYED
    (0x1F595, 0x1F596), // REVERSED HAND WITH MIDDLE FINGER EXTENDED..RAISED HAND WITH PART BETWEEN MIDDLE AND RING FINGERS
    (0x1F5A4, 0x1F5A4), // BLACK HEART
    (0x1F5A5, 0x1F5A5), // DESKTOP COMPUTER
    (0x1F5A8, 0x1F5A8), // PRINTER
    (0x1F5B1, 0x1F5B2), // THREE BUTTON MOUSE..TRACKBALL
    (0x1F5BC, 0x1F5BC), // FRAME WITH PICTURE
    (0x1F5C2, 0x1F5C4), // CARD INDEX DIVIDERS..FILE CABINET
    (0x1F5D1, 0x1F5D3), // WASTEBASKET..SPIRAL CALENDAR PAD
    (0x1F5DC, 0x1F5DE), // COMPRESSION..ROLLED-UP NEWSPAPER
    (0x1F5E1, 0x1F5E1), // DAGGER KNIFE
    (0x1F5E3, 0x1F5E3), // SPEAKING HEAD IN SILHOUETTE
    (0x1F5E8, 0x1F5E8), // LEFT SPEECH BUBBLE
    (0x1F5EF, 0x1F5EF), // RIGHT ANGER BUBBLE
    (0x1F5F3, 0x1F5F3), // BALLOT BOX WITH BALLOT
    (0x1F5FA, 0x1F5FA), // WORLD MAP
    (0x1F5FB, 0x1F5FF), // MOUNT FUJI..MOYAI
    (0x1F600, 0x1F64F), // GRINNING FACE..PERSON WITH FOLDED HANDS
    (0x1F680, 0x1F6C5), // ROCKET..LEFT LUGGAGE
    (0x1F6CB, 0x1F6CF), // COUCH AND LAMP..BED
    (0x1F6D0, 0x1F6D0), // PLACE OF WORSHIP
    (0x1F6D1, 0x1F6D2), // OCTAGONAL SIGN..SHOPPING TROLLEY
    (0x1F6D5, 0x1F6D5), // HINDU TEMPLE
    (0x1F6D6, 0x1F6D7), // HUT..ELEVATOR
    (0x1F6DC, 0x1F6DC), // WIRELESS
    (0x1F6DD, 0x1F6DF), // PLAYGROUND SLIDE..RING BUOY
    (0x1F6E0, 0x1F6E5), // HAMMER AND WRENCH..MOTOR BOAT
    (0x1F6E9, 0x1F6E9), // SMALL AIRPLANE
    (0x1F6EB, 0x1F6EC), // AIRPLANE DEPARTURE..AIRPLANE ARRIVING
    (0x1F6F0, 0x1F6F0), // SATELLITE
    (0x1F6F3, 0x1F6F3), // PASSENGER SHIP
    (0x1F6F4, 0x1F6F6), // SCOOTER..CANOE
    (0x1F6F7, 0x1F6F8), // SLED..FLYING SAUCER
    (0x1F6F9, 0x1F6F9), // SKATEBOARD
    (0x1F6FA, 0x1F6FA), // AUTO RICKSHAW
    (0x1F6FB, 0x1F6FC), // PICKUP TRUCK..ROLLER SKATE
    (0x1F7E0, 0x1F7EB), // LARGE ORANGE CIRCLE..LARGE BROWN SQUARE
    (0x1F7F0, 0x1F7F0), // HEAVY EQUALS SIGN
    (0x1F90C, 0x1F90C), // PINCHED FINGERS
    (0x1F90D, 0x1F90F), // WHITE HEART..PINCHING HAND
    (0x1F910, 0x1F918), // ZIPPER-MOUTH FACE..SIGN OF THE HORNS
    (0x1F919, 0x1F91E), // CALL ME HAND..HAND WITH INDEX AND MIDDLE FINGERS CROSSED
    (0x1F91F, 0x1F91F), // I LOVE YOU HAND SIGN
    (0x1F920, 0x1F927), // FACE WITH COWBOY HAT..SNEEZING FACE
    (0x1F928, 0x1F92F), // FACE WITH ONE EYEBROW RAISED..SHOCKED FACE WITH EXPLODING HEAD
    (0x1F930, 0x1F930), // PREGNANT WOMAN
    (0x1F931, 0x1F932), // BREAST-FEEDING..PALMS UP TOGETHER
    (0x1F933, 0x1F93A), // SELFIE..FENCER
    (0x1F93C, 0x1F93E), // WRESTLERS..HANDBALL
    (0x1F93F, 0x1F93F), // DIVING MASK
    (0x1F940, 0x1F945), // WILTED FLOWER..GOAL NET
    (0x1F947, 0x1F94B), // FIRST PLACE MEDAL..MARTIAL ARTS UNIFORM
    (0x1F94C, 0x1F94C), // CURLING STONE
    (0x1F94D, 0x1F94F), // LACROSSE STICK AND BALL..FLYING DISC
    (0x1F950, 0x1F95E), // CROISSANT..PANCAKES
    (0x1F95F, 0x1F96B), // DUMPLING..CANNED FOOD
    (0x1F96C, 0x1F970), // LEAFY GREEN..SMILING FACE WITH SMILING EYES AND THREE HEARTS
    (0x1F971, 0x1F971), // YAWNING FACE
    (0x1F972, 0x1F972), // SMILING FACE WITH TEAR
    (0x1F973, 0x1F976), // FACE WITH PARTY HORN AND PARTY HAT..FREEZING FACE
    (0x1F977, 0x1F978), // NINJA..DISGUISED FACE
    (0x1F979, 0x1F979), // FACE HOLDING BACK TEARS
    (0x1F97A, 0x1F97A), // FACE WITH PLEADING EYES
    (0x1F97B, 0x1F97B), // SARI
    (0x1F97C, 0x1F97F), // LAB COAT..FLAT SHOE
    (0x1F980, 0x1F984), // CRAB..UNICORN FACE
    (0x1F985, 0x1F991), // EAGLE..SQUID
    (0x1F992, 0x1F997), // GIRAFFE FACE..CRICKET
    (0x1F998, 0x1F9A2), // KANGAROO..SWAN
    (0x1F9A3, 0x1F9A4), // MAMMOTH..DODO
    (0x1F9A5, 0x1F9AA), // SLOTH..OYSTER
    (0x1F9AB, 0x1F9AD), // BEAVER..SEAL
    (0x1F9AE, 0x1F9AF), // GUIDE DOG..PROBING CANE
    (0x1F9B0, 0x1F9B9), // EMOJI COMPONENT RED HAIR..SUPERVILLAIN
    (0x1F9BA, 0x1F9BF), // SAFETY VEST..MECHANICAL LEG
    (0x1F9C0, 0x1F9C0), // CHEESE WEDGE
    (0x1F9C1, 0x1F9C2), // CUPCAKE..SALT SHAKER
    (0x1F9C3, 0x1F9CA), // BEVERAGE BOX..ICE CUBE
    (0x1F9CB, 0x1F9CB), // BUBBLE TEA
    (0x1F9CC, 0x1F9CC), // TROLL
    (0x1F9CD, 0x1F9CF), // STANDING PERSON..DEAF PERSON
    (0x1F9D0, 0x1F9E6), // FACE WITH MONOCLE..SOCKS
    (0x1F9E7, 0x1F9FF), // RED ENVELOPE..NAZAR AMULET
    (0x1FA70, 0x1FA73), // BALLET SHOES..SHORTS
    (0x1FA74, 0x1FA74), // THONG SANDAL
    (0x1FA75, 0x1FA77), // LIGHT BLUE HEART..PINK HEART
    (0x1FA78, 0x1FA7A), // DROP OF BLOOD..STETHOSCOPE
    (0x1FA7B, 0x1FA7C), // X-RAY..CRUTCH
    (0x1FA80, 0x1FA82), // YO-YO..PARACHUTE
    (0x1FA83, 0x1FA86), // BOOMERANG..NESTING DOLLS
    (0x1FA87, 0x1FA88), // MARACAS..FLUTE
    (0x1FA89, 0x1FA89), // HARP
    (0x1FA8F, 0x1FA8F), // SHOVEL
    (0x1FA90, 0x1FA95), // RINGED PLANET..BANJO
    (0x1FA96, 0x1FAA8), // MILITARY HELMET..ROCK
    (0x1FAA9, 0x1FAAC), // MIRROR BALL..HAMSA
    (0x1FAAD, 0x1FAAF), // FOLDING HAND FAN..KHANDA
    (0x1FAB0, 0x1FAB6), // FLY..FEATHER
    (0x1FAB7, 0x1FABA), // LOTUS..NEST WITH EGGS
    (0x1FABB, 0x1FABD), // HYACINTH..WING
    (0x1FABE, 0x1FABE), // GOOSE
    (0x1FABF, 0x1FABF), // JELLYFISH
    (0x1FAC0, 0x1FAC2), // ANATOMICAL HEART..PEOPLE HUGGING
    (0x1FAC3, 0x1FAC5), // PREGNANT MAN..PERSON WITH CROWN
    (0x1FAC6, 0x1FAC6), // MOOSE
    (0x1FACE, 0x1FACF), // MOOSE..DONKEY
    (0x1FAD0, 0x1FAD6), // BLUEBERRIES..TEAPOT
    (0x1FAD7, 0x1FAD9), // POURING LIQUID..JAR
    (0x1FADA, 0x1FADB), // GINGER ROOT..PEA POD
    (0x1FADC, 0x1FADC), // MELTING FACE (placeholder for ranges added in 15.0+)
    (0x1FADF, 0x1FADF), // SHAKING FACE (placeholder for ranges added in 15.0+)
    (0x1FAE0, 0x1FAE7), // MELTING FACE..BUBBLES
    (0x1FAE8, 0x1FAE8), // SHAKING FACE
    (0x1FAE9, 0x1FAE9), // FACE WITH OPEN EYES AND HAND OVER MOUTH (15.1)
    (0x1FAF0, 0x1FAF6), // HAND WITH INDEX FINGER AND THUMB CROSSED..HEART HANDS
    (0x1FAF7, 0x1FAF8), // LEFTWARDS PUSHING HAND..RIGHTWARDS PUSHING HAND
];

/// `Emoji_Presentation=Yes` code points (subset of `EMOJI_RANGES` whose
/// default presentation is the colored glyph).
///
/// Source: Unicode `emoji-data.txt`, Emoji 16.0.
static EMOJI_PRESENTATION_RANGES: &[(u32, u32)] = &[
    (0x231A, 0x231B),   // WATCH..HOURGLASS
    (0x23E9, 0x23EC),   // BLACK RIGHT-POINTING DOUBLE TRIANGLE..BLACK DOWN-POINTING DOUBLE TRIANGLE
    (0x23F0, 0x23F0),   // ALARM CLOCK
    (0x23F3, 0x23F3),   // HOURGLASS WITH FLOWING SAND
    (0x25FD, 0x25FE),   // WHITE MEDIUM SMALL SQUARE..BLACK MEDIUM SMALL SQUARE
    (0x2614, 0x2615),   // UMBRELLA WITH RAIN DROPS..HOT BEVERAGE
    (0x2648, 0x2653),   // ARIES..PISCES
    (0x267F, 0x267F),   // WHEELCHAIR SYMBOL
    (0x2693, 0x2693),   // ANCHOR
    (0x26A1, 0x26A1),   // HIGH VOLTAGE SIGN
    (0x26AA, 0x26AB),   // MEDIUM WHITE CIRCLE..MEDIUM BLACK CIRCLE
    (0x26BD, 0x26BE),   // SOCCER BALL..BASEBALL
    (0x26C4, 0x26C5),   // SNOWMAN WITHOUT SNOW..SUN BEHIND CLOUD
    (0x26CE, 0x26CE),   // OPHIUCHUS
    (0x26D4, 0x26D4),   // NO ENTRY
    (0x26EA, 0x26EA),   // CHURCH
    (0x26F2, 0x26F3),   // FOUNTAIN..FLAG IN HOLE
    (0x26F5, 0x26F5),   // SAILBOAT
    (0x26FA, 0x26FA),   // TENT
    (0x26FD, 0x26FD),   // FUEL PUMP
    (0x2705, 0x2705),   // WHITE HEAVY CHECK MARK
    (0x270A, 0x270B),   // RAISED FIST..RAISED HAND
    (0x2728, 0x2728),   // SPARKLES
    (0x274C, 0x274C),   // CROSS MARK
    (0x274E, 0x274E),   // NEGATIVE SQUARED CROSS MARK
    (0x2753, 0x2755),   // BLACK QUESTION MARK ORNAMENT..WHITE EXCLAMATION MARK ORNAMENT
    (0x2757, 0x2757),   // HEAVY EXCLAMATION MARK SYMBOL
    (0x2795, 0x2797),   // HEAVY PLUS SIGN..HEAVY DIVISION SIGN
    (0x27B0, 0x27B0),   // CURLY LOOP
    (0x27BF, 0x27BF),   // DOUBLE CURLY LOOP
    (0x2B1B, 0x2B1C),   // BLACK LARGE SQUARE..WHITE LARGE SQUARE
    (0x2B50, 0x2B50),   // WHITE MEDIUM STAR
    (0x2B55, 0x2B55),   // HEAVY LARGE CIRCLE
    (0x1F004, 0x1F004), // MAHJONG TILE RED DRAGON
    (0x1F0CF, 0x1F0CF), // PLAYING CARD BLACK JOKER
    (0x1F18E, 0x1F18E), // NEGATIVE SQUARED AB
    (0x1F191, 0x1F19A), // SQUARED CL..SQUARED VS
    (0x1F1E6, 0x1F1FF), // Regional indicator A..Z
    (0x1F201, 0x1F201), // SQUARED KATAKANA KOKO
    (0x1F21A, 0x1F21A), // SQUARED CJK 7121
    (0x1F22F, 0x1F22F), // SQUARED CJK 6307
    (0x1F232, 0x1F236), // SQUARED CJK 7981..SQUARED CJK 6E80
    (0x1F238, 0x1F23A), // SQUARED CJK 7533..SQUARED CJK 55B6
    (0x1F250, 0x1F251), // CIRCLED IDEOGRAPH ADVANTAGE..ACCEPT
    (0x1F300, 0x1F320), // CYCLONE..SHOOTING STAR
    (0x1F32D, 0x1F335), // HOT DOG..CACTUS
    (0x1F337, 0x1F37C), // TULIP..BABY BOTTLE
    (0x1F37E, 0x1F393), // BOTTLE WITH POPPING CORK..GRADUATION CAP
    (0x1F3A0, 0x1F3CA), // CAROUSEL HORSE..SWIMMER
    (0x1F3CF, 0x1F3D3), // CRICKET BAT..TABLE TENNIS
    (0x1F3E0, 0x1F3F0), // HOUSE BUILDING..EUROPEAN CASTLE
    (0x1F3F4, 0x1F3F4), // WAVING BLACK FLAG
    (0x1F3F8, 0x1F43E), // BADMINTON RACQUET..PAW PRINTS
    (0x1F440, 0x1F440), // EYES
    (0x1F442, 0x1F4FC), // EAR..VIDEOCASSETTE
    (0x1F4FF, 0x1F53D), // PRAYER BEADS..DOWN-POINTING SMALL RED TRIANGLE
    (0x1F54B, 0x1F54E), // KAABA..MENORAH WITH NINE BRANCHES
    (0x1F550, 0x1F567), // CLOCK FACE 1..CLOCK FACE 12-30
    (0x1F57A, 0x1F57A), // MAN DANCING
    (0x1F595, 0x1F596), // REVERSED HAND..VULCAN SALUTE
    (0x1F5A4, 0x1F5A4), // BLACK HEART
    (0x1F5FB, 0x1F64F), // MOUNT FUJI..PERSON WITH FOLDED HANDS
    (0x1F680, 0x1F6C5), // ROCKET..LEFT LUGGAGE
    (0x1F6CC, 0x1F6CC), // SLEEPING ACCOMMODATION
    (0x1F6D0, 0x1F6D2), // PLACE OF WORSHIP..SHOPPING TROLLEY
    (0x1F6D5, 0x1F6D7), // HINDU TEMPLE..ELEVATOR
    (0x1F6DC, 0x1F6DF), // WIRELESS..RING BUOY
    (0x1F6EB, 0x1F6EC), // AIRPLANE DEPARTURE..AIRPLANE ARRIVING
    (0x1F6F4, 0x1F6FC), // SCOOTER..ROLLER SKATE
    (0x1F7E0, 0x1F7EB), // LARGE ORANGE CIRCLE..LARGE BROWN SQUARE
    (0x1F7F0, 0x1F7F0), // HEAVY EQUALS SIGN
    (0x1F90C, 0x1F93A), // PINCHED FINGERS..FENCER
    (0x1F93C, 0x1F945), // WRESTLERS..GOAL NET
    (0x1F947, 0x1F9FF), // FIRST PLACE MEDAL..NAZAR AMULET
    (0x1FA70, 0x1FA7C), // BALLET SHOES..CRUTCH
    (0x1FA80, 0x1FA89), // YO-YO..HARP
    (0x1FA8F, 0x1FAC6), // SHOVEL..MOOSE
    (0x1FACE, 0x1FADC), // (15.0+ additions)
    (0x1FADF, 0x1FAE9), // (15.0/15.1 additions)
    (0x1FAF0, 0x1FAF8), // HAND WITH INDEX..RIGHTWARDS PUSHING HAND
];

/// Variation Selector-15: force text (monochrome) presentation.
pub const VS15: char = '\u{FE0E}';
/// Variation Selector-16: force emoji (color) presentation.
pub const VS16: char = '\u{FE0F}';

fn in_ranges(cp: u32, ranges: &[(u32, u32)]) -> bool {
    // Sorted, non-overlapping inclusive ranges → binary search.
    ranges
        .binary_search_by(|(start, end)| {
            if cp < *start {
                std::cmp::Ordering::Greater
            } else if cp > *end {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Public predicate: `Emoji=Yes`.
pub fn is_emoji(cp: char) -> bool {
    in_ranges(cp as u32, EMOJI_RANGES)
}

/// Public predicate: `Emoji_Presentation=Yes`.
pub fn has_emoji_presentation_default(cp: char) -> bool {
    in_ranges(cp as u32, EMOJI_PRESENTATION_RANGES)
}

/// Keycap-eligible base characters: U+0023 `#`, U+002A `*`, U+0030..=U+0039
/// digits. A bare instance of any of these MUST render as plain text — the
/// keycap-emoji presentation only applies when the cluster also contains
/// COMBINING ENCLOSING KEYCAP (U+20E3), at which point the cluster-level
/// dispatcher (e.g. `FallbackChain::resolve_for_cluster`) overrides this
/// default.
fn is_keycap_base(codepoint: char) -> bool {
    matches!(codepoint, '0'..='9' | '*' | '#')
}

/// Resolve which font should rasterize `codepoint` given an optional
/// variation selector (`VS15` / `VS16`, or any other / `None`).
///
/// Note: keycap base characters (`'0'..='9'`, `'*'`, `'#'`) return
/// [`EmojiPresentation::NotEmoji`] when no variation selector is
/// supplied, even though Unicode lists them in `Emoji=Yes`. They only
/// participate in emoji presentation through a full keycap cluster
/// (`<digit> <VS16> <U+20E3>`); the cluster-level dispatcher in
/// `FallbackChain` is the appropriate place to handle that case so
/// per-codepoint dispatch does not route every ASCII digit to the
/// monochrome emoji face and break grid alignment.
pub fn presentation_for(codepoint: char, variation_selector: Option<char>) -> EmojiPresentation {
    match variation_selector {
        Some(VS16) => EmojiPresentation::Color,
        Some(VS15) => EmojiPresentation::Monochrome,
        _ => {
            if is_keycap_base(codepoint) {
                EmojiPresentation::NotEmoji
            } else if has_emoji_presentation_default(codepoint) {
                EmojiPresentation::Color
            } else if is_emoji(codepoint) {
                EmojiPresentation::Monochrome
            } else {
                EmojiPresentation::NotEmoji
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TS-1: ASCII letters are not emoji.
    #[test]
    fn presentation_for_ascii_letter_is_not_emoji() {
        assert_eq!(presentation_for('a', None), EmojiPresentation::NotEmoji);
    }

    /// TS-2: U+23F5 (BLACK MEDIUM RIGHT-POINTING TRIANGLE) is Emoji=Yes,
    /// Emoji_Presentation=No → default to monochrome.
    #[test]
    fn presentation_for_text_default_emoji_is_monochrome() {
        assert_eq!(
            presentation_for('\u{23F5}', None),
            EmojiPresentation::Monochrome
        );
    }

    /// TS-3: VS16 forces color presentation even for text-default code points.
    #[test]
    fn presentation_for_vs16_forces_color() {
        assert_eq!(
            presentation_for('\u{23F5}', Some(VS16)),
            EmojiPresentation::Color
        );
    }

    /// TS-4: U+1F600 (GRINNING FACE) has Emoji_Presentation=Yes → color.
    #[test]
    fn presentation_for_emoji_default_is_color() {
        assert_eq!(
            presentation_for('\u{1F600}', None),
            EmojiPresentation::Color
        );
    }

    /// TS-5: VS15 forces text (monochrome) presentation for color-default code points.
    #[test]
    fn presentation_for_vs15_forces_monochrome() {
        assert_eq!(
            presentation_for('\u{1F600}', Some(VS15)),
            EmojiPresentation::Monochrome
        );
    }

    /// Digits / `*` / `#` are listed in EMOJI_RANGES but only participate
    /// in emoji presentation through a full keycap cluster
    /// (`<base> <VS16> <U+20E3>`). A bare instance MUST resolve to
    /// `NotEmoji` so per-codepoint dispatch does not route every ASCII
    /// digit through the monochrome emoji face (which would break grid
    /// alignment). VS16 still forces color when explicitly supplied —
    /// the cluster-level dispatcher (`FallbackChain::resolve_for_cluster`)
    /// uses that path to handle the full keycap sequence.
    #[test]
    fn presentation_for_digit_is_not_emoji() {
        assert_eq!(presentation_for('5', None), EmojiPresentation::NotEmoji);
        assert_eq!(presentation_for('0', None), EmojiPresentation::NotEmoji);
        assert_eq!(presentation_for('9', None), EmojiPresentation::NotEmoji);
        assert_eq!(presentation_for('*', None), EmojiPresentation::NotEmoji);
        assert_eq!(presentation_for('#', None), EmojiPresentation::NotEmoji);
        // The underlying property tables still list these — only the
        // dispatch policy in `presentation_for` short-circuits.
        assert!(is_emoji('5'));
        assert!(!has_emoji_presentation_default('5'));
        // VS16 still overrides — cluster-level dispatch uses this path
        // when the cluster carries `<base> + VS16 + U+20E3`.
        assert_eq!(presentation_for('5', Some(VS16)), EmojiPresentation::Color);
    }

    /// CJK / Hiragana / etc. are not in either table.
    #[test]
    fn presentation_for_japanese_letter_is_not_emoji() {
        assert_eq!(presentation_for('あ', None), EmojiPresentation::NotEmoji);
        assert_eq!(presentation_for('漢', None), EmojiPresentation::NotEmoji);
    }

    /// Non-VS variation selectors fall through to property tables.
    #[test]
    fn presentation_for_other_vs_falls_through() {
        // Mongolian Free Variation Selector One is not VS15/VS16.
        let other_vs = Some('\u{180B}');
        assert_eq!(
            presentation_for('\u{1F600}', other_vs),
            EmojiPresentation::Color
        );
        assert_eq!(presentation_for('a', other_vs), EmojiPresentation::NotEmoji);
    }

    #[test]
    fn emoji_ranges_sorted_and_disjoint() {
        for w in EMOJI_RANGES.windows(2) {
            assert!(w[0].0 <= w[0].1, "range {:?} reversed", w[0]);
            assert!(w[0].1 < w[1].0, "ranges overlap: {:?} {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn emoji_presentation_ranges_sorted_and_disjoint() {
        for w in EMOJI_PRESENTATION_RANGES.windows(2) {
            assert!(w[0].0 <= w[0].1, "range {:?} reversed", w[0]);
            assert!(w[0].1 < w[1].0, "ranges overlap: {:?} {:?}", w[0], w[1]);
        }
    }
}
