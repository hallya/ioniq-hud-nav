import SwiftUI
import UIKit

/// Step 1 screen: Bluelink account, the two test runs, and the one line the
/// user copies back into the chat.
struct DiagnosticView: View {
    @StateObject private var model = DiagnosticModel()
    @State private var copied = false

    var body: some View {
        NavigationStack {
            List {
                Section("Status") {
                    Text(model.line)
                        .font(.system(.body, design: .monospaced))
                        .textSelection(.enabled)
                    Button(copied ? "Copied" : "Copy status line") {
                        UIPasteboard.general.string = model.line
                        copied = true
                    }
                    if let note = model.note {
                        Text(note).font(.footnote).foregroundStyle(.secondary)
                    }
                }

                Section("Test") {
                    Button("Send test destination") { start(send: true) }
                    Button("Check connection only") { start(send: false) }
                    if model.repairOffered {
                        Button("Update signature and retry") {
                            copied = false
                            Task { await model.repairAndRetry() }
                        }
                    }
                    if model.running {
                        ProgressView()
                    }
                }
                .disabled(model.running)

                Section {
                    TextField("Email", text: $model.email)
                        .textContentType(.username)
                        .keyboardType(.emailAddress)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    SecureField(model.passwordSaved ? "Password (saved)" : "Password", text: $model.password)
                        .textContentType(.password)
                    SecureField(model.pinSaved ? "PIN (saved)" : "Bluelink PIN", text: $model.pin)
                        .keyboardType(.numberPad)
                    Button("Save to Keychain") { model.saveCredentials() }
                } header: {
                    Text("Bluelink account")
                } footer: {
                    Text("Stored in this iPhone's Keychain only. Empty fields keep the saved value.")
                }

                Section("Core") {
                    LabeledContent("Rust core", value: coreVersion())
                    LabeledContent("Paris to Lyon", value: parisLyonKm())
                }
            }
            .navigationTitle("Diagnostic")
        }
    }

    private func start(send: Bool) {
        copied = false
        Task { await model.run(send: send) }
    }

    private func parisLyonKm() -> String {
        let metres = distanceM(aLat: 48.8566, aLon: 2.3522, bLat: 45.7640, bLon: 4.8357)
        return String(format: "%.0f km", metres / 1000)
    }
}

@MainActor
final class DiagnosticModel: ObservableObject {
    @Published var line = diagnosticLine(auth: .pending, vehicle: .pending, soc: nil, send: .pending, planMs: nil)
    @Published var note: String?
    @Published var running = false
    @Published var repairOffered = false

    @Published var email = Keychain.read(Keychain.email) ?? ""
    @Published var password = ""
    @Published var pin = ""
    @Published var passwordSaved = Keychain.read(Keychain.password) != nil
    @Published var pinSaved = Keychain.read(Keychain.pin) != nil

    private var lastSend = true

    func saveCredentials() {
        var ok = Keychain.save(email.trimmingCharacters(in: .whitespaces), for: Keychain.email)
        if !password.isEmpty { ok = Keychain.save(password, for: Keychain.password) && ok }
        if !pin.isEmpty { ok = Keychain.save(pin, for: Keychain.pin) && ok }
        password = ""
        pin = ""
        passwordSaved = Keychain.read(Keychain.password) != nil
        pinSaved = Keychain.read(Keychain.pin) != nil
        note = ok ? "Saved." : "Keychain error: not saved."
    }

    func run(send: Bool) async {
        guard let email = Keychain.read(Keychain.email), !email.isEmpty,
              let password = Keychain.read(Keychain.password), !password.isEmpty
        else {
            note = "Save your Bluelink email and password first."
            return
        }
        let pin = Keychain.read(Keychain.pin) ?? ""
        if send && pin.isEmpty {
            note = "Save your Bluelink PIN first: sending a destination needs it."
            return
        }
        lastSend = send
        running = true
        note = send ? "Running, then check the car's screen." : "Running."
        let outcome = await BluelinkClient.run(
            Credentials(email: email, password: password, pin: pin),
            destination: send ? [testDestination()] : []
        )
        running = false
        line = outcome.line
        repairOffered = outcome.repairSignature
        note = outcome.allOk ? "All ok." : nil
    }

    func repairAndRetry() async {
        running = true
        note = "Updating signature."
        let result = await SignatureRepair.update(for: BundledConstants.text)
        running = false
        guard result == "ok" else {
            note = "Signature update failed (\(result)). Use myHyundai's send-to-car for now."
            return
        }
        repairOffered = false
        await run(send: lastSend)
    }
}
