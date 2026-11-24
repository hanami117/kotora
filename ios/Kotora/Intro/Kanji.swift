import SwiftUI

/// 小虎 as the desktop draws it (`app/src/kanji.rs`): Noto Sans JP's outlines at weight 700 (SIL Open
/// Font License 1.1, `assets/kanji/NotoSansJP-OFL.txt`), as SVG paths in a 100 box per glyph,
/// baseline at 88. Vectors, so it is the same at any size and needs no CJK font.
enum Kanji {
    static let ko = "M43.8 4.4H56.9V81.9Q56.9 87.5 55.5 90.3Q54.1 93.1 50.7 94.6Q47.2 96 41.9 96.5Q36.7 96.9 29.4 96.8Q29.1 95.1 28.3 92.8Q27.5 90.6 26.5 88.3Q25.6 86 24.6 84.4Q27.9 84.5 31.2 84.6Q34.5 84.7 37.1 84.6Q39.7 84.6 40.8 84.6Q42.4 84.6 43.1 84Q43.8 83.4 43.8 81.9ZM67.8 30.7 79.6 26.3Q83.7 33.4 87.6 41.5Q91.4 49.5 94.4 57.2Q97.3 65 98.6 71.3L85.4 76.5Q84.4 70.4 81.8 62.5Q79.1 54.7 75.4 46.3Q71.8 38 67.8 30.7ZM17.6 27.4 31.2 29.7Q29.5 37.1 27.1 45.5Q24.7 53.8 21.4 61.4Q18.2 68.9 14 74.5Q12.5 73.4 10.4 72.2Q8.2 71 6.1 70Q3.9 68.9 2.2 68.2Q6.3 63.1 9.3 56.1Q12.4 49 14.5 41.4Q16.6 33.9 17.6 27.4Z"
    static let tora = "M48.5 9.4H87.1V19.1H48.5ZM19.5 23.2H86.8V33.3H19.5ZM43.1 3H55.2V26.3H43.1ZM26.2 40.4 75.8 36.4 76.8 44.6 27.2 48.7ZM43.1 31.1H54.4V47.4Q54.4 49.6 55.6 50.2Q56.9 50.8 61 50.8Q61.8 50.8 63.5 50.8Q65.3 50.8 67.4 50.8Q69.6 50.8 71.8 50.8Q73.9 50.8 75.8 50.8Q77.7 50.8 78.6 50.8Q80.7 50.8 81.8 50.3Q82.9 49.8 83.4 48.5Q83.9 47.2 84.2 44.6Q85.8 45.7 88.8 46.7Q91.7 47.7 93.9 48.1Q93.3 52.7 91.8 55.3Q90.3 57.9 87.5 58.9Q84.6 59.8 79.8 59.8Q78.9 59.8 76.9 59.8Q74.9 59.8 72.4 59.8Q69.9 59.8 67.3 59.8Q64.8 59.8 62.9 59.8Q60.9 59.8 60 59.8Q53.3 59.8 49.6 58.8Q46 57.7 44.6 55Q43.1 52.3 43.1 47.6ZM11.9 23.2H23.7V46.3Q23.7 51.9 23.4 58.5Q23 65.1 21.9 72Q20.9 78.9 18.9 85.3Q16.8 91.8 13.6 97.1Q12.5 96.1 10.6 94.9Q8.7 93.7 6.7 92.5Q4.7 91.4 3.2 90.9Q6.2 86.1 8 80.4Q9.7 74.7 10.6 68.7Q11.4 62.7 11.7 56.9Q11.9 51.2 11.9 46.3ZM81.9 23.2H83.8L85.9 22.6L95.3 25.3Q94 30.2 92.2 35.2Q90.5 40.3 88.7 43.9L78 40Q79.1 37 80.2 32.8Q81.3 28.6 81.9 24.9ZM58.9 62.1H70.3V81.6Q70.3 83.7 70.9 84.3Q71.5 84.8 73.7 84.8Q74.2 84.8 75.3 84.8Q76.4 84.8 77.8 84.8Q79.1 84.8 80.3 84.8Q81.5 84.8 82.1 84.8Q83.5 84.8 84.2 84.1Q84.9 83.3 85.2 81Q85.6 78.6 85.8 73.7Q86.9 74.7 88.7 75.6Q90.5 76.4 92.5 77.1Q94.5 77.8 96 78.1Q95.4 85 94 88.8Q92.7 92.6 90.1 94Q87.6 95.5 83.3 95.5Q82.4 95.5 80.9 95.5Q79.3 95.5 77.6 95.5Q75.9 95.5 74.4 95.5Q72.9 95.5 72 95.5Q66.7 95.5 63.9 94.2Q61 92.9 59.9 89.9Q58.9 86.8 58.9 81.7ZM36.9 62H48.2V70.6Q48.2 73.7 47.5 77.3Q46.7 80.9 44.5 84.6Q42.3 88.3 38.1 91.5Q33.9 94.7 27.2 97.2Q26.2 96.2 24.7 94.8Q23.2 93.4 21.6 92.1Q20 90.8 18.7 89.9Q24.7 88 28.3 85.5Q31.9 83 33.8 80.3Q35.6 77.6 36.3 75Q36.9 72.4 36.9 70.3Z"

    /// The glyph's outline scaled to `size` points (one em), its box at `origin`.
    static func path(_ d: String, size: CGFloat, origin: CGPoint) -> Path {
        let k = size / 100
        var p = Path()
        var nums: [CGFloat] = []
        var cmd: Character = "M"
        var cur = CGPoint.zero
        func pt(_ x: CGFloat, _ y: CGFloat) -> CGPoint { CGPoint(x: origin.x + x * k, y: origin.y + y * k) }
        func flush() {
            switch cmd {
            case "M", "L":
                var i = 0
                while i + 1 < nums.count {
                    cur = CGPoint(x: nums[i], y: nums[i + 1])
                    if cmd == "M" && i == 0 { p.move(to: pt(cur.x, cur.y)) } else { p.addLine(to: pt(cur.x, cur.y)) }
                    i += 2
                }
            case "H":
                for x in nums { cur.x = x; p.addLine(to: pt(cur.x, cur.y)) }
            case "V":
                for y in nums { cur.y = y; p.addLine(to: pt(cur.x, cur.y)) }
            case "Q":
                var i = 0
                while i + 3 < nums.count {
                    let c = pt(nums[i], nums[i + 1])
                    cur = CGPoint(x: nums[i + 2], y: nums[i + 3])
                    p.addQuadCurve(to: pt(cur.x, cur.y), control: c)
                    i += 4
                }
            case "Z":
                p.closeSubpath()
            default: break
            }
            nums.removeAll()
        }
        var token = ""
        func endNumber() {
            if let v = Double(token) { nums.append(CGFloat(v)) }
            token = ""
        }
        for ch in d {
            if ch.isLetter {
                endNumber(); flush(); cmd = ch
            } else if ch == " " || ch == "," {
                endNumber()
            } else if ch == "-" && !token.isEmpty {
                endNumber(); token = "-"
            } else {
                token.append(ch)
            }
        }
        endNumber(); flush()
        return p
    }

    /// 小虎 side by side, `size` points per glyph, centred on `center`.
    static func name(size: CGFloat, center: CGPoint) -> Path {
        let o = CGPoint(x: center.x - size, y: center.y - size / 2)
        var p = path(ko, size: size, origin: o)
        p.addPath(path(tora, size: size, origin: CGPoint(x: o.x + size, y: o.y)))
        return p
    }
}
