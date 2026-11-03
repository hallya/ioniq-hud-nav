import SwiftUI
import UIKit

/// Step 1 screen: proves the Rust core runs on the phone and shows the one line
/// the user copies back into the chat. Bluelink checks are wired in next.
struct DiagnosticView: View {
    @State private var copied = false

    private var line: String {
        diagnosticLine(auth: .pending, vehicle: .pending, soc: nil, send: .pending, planMs: nil)
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Status") {
                    Text(line)
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                    Button(copied ? "Copied" : "Copy status line") {
                        UIPasteboard.general.string = line
                        copied = true
                    }
                }
                Section("Core") {
                    LabeledContent("Rust core", value: coreVersion())
                    LabeledContent("Paris to Lyon", value: parisLyonKm())
                }
            }
            .navigationTitle("Diagnostic")
        }
    }

    private func parisLyonKm() -> String {
        let metres = distanceM(aLat: 48.8566, aLon: 2.3522, bLat: 45.7640, bLon: 4.8357)
        return String(format: "%.0f km", metres / 1000)
    }
}
