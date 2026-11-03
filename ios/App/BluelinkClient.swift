import CryptoKit
import Foundation
import Security

struct Credentials {
    var email: String
    var password: String
    /// Only needed to send a destination.
    var pin: String
}

struct RunOutcome {
    let line: String
    let allOk: Bool
    let repairSignature: Bool
}

/// Performs the effects of a core `BluelinkRun`: HTTP, RSA, clock and
/// randomness. All protocol logic is in the Rust core.
enum BluelinkClient {
    /// `destination` empty: connection check, nothing is sent to the car.
    static func run(_ credentials: Credentials, destination: [Destination]) async -> RunOutcome {
        let constants = BundledConstants.text
        let run = BluelinkRun(
            constantsJson: constants,
            overrideJson: SignatureRepair.storedOverride(for: constants),
            username: credentials.email,
            passwordChars: UInt32(credentials.password.unicodeScalars.count),
            pin: credentials.pin,
            language: language,
            timezone: timezone,
            entropy: randomBytes(Int(bluelinkEntropyLen())),
            destination: destination
        )
        // One run, one login, one in-memory cookie store.
        let session = URLSession(configuration: .ephemeral)
        defer { session.finishTasksAndInvalidate() }

        var step = run.start(nowMs: nowMs())
        while true {
            switch step {
            case let .send(request):
                let input = await perform(request, in: session)
                step = run.resume(input: input, nowMs: nowMs())
            case let .encryptPassword(publicKeyDer):
                let ciphertext = encrypt(credentials.password, with: publicKeyDer) ?? Data()
                step = run.resume(input: .encrypted(ciphertext: ciphertext), nowMs: nowMs())
            case let .done(line, allOk, repairSignature):
                return RunOutcome(line: line, allOk: allOk, repairSignature: repairSignature)
            }
        }
    }

    private static func perform(_ r: HttpRequest, in session: URLSession) async -> RunInput {
        guard let url = URL(string: r.url) else { return .failed(code: 0) }
        var request = URLRequest(url: url, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 30)
        request.httpMethod = r.method
        for header in r.headers {
            request.addValue(header.value, forHTTPHeaderField: header.name)
        }
        request.httpBody = r.body.map { Data($0.utf8) }
        let delegate: URLSessionTaskDelegate? = r.followRedirects ? nil : NoRedirect.shared
        do {
            let (data, response) = try await session.data(for: request, delegate: delegate)
            guard let http = response as? HTTPURLResponse else { return .failed(code: 0) }
            let headers = http.allHeaderFields.compactMap { key, value -> Header? in
                guard let name = key as? String else { return nil }
                return Header(name: name, value: "\(value)")
            }
            return .received(response: HttpResponse(
                status: UInt16(clamping: http.statusCode),
                headers: headers,
                url: http.url?.absoluteString ?? r.url,
                body: String(decoding: data, as: UTF8.self)
            ))
        } catch let error as URLError {
            return .failed(code: Int32(clamping: error.errorCode))
        } catch {
            return .failed(code: 0)
        }
    }

    /// RSA PKCS#1 v1.5 under a PKCS#1 `RSAPublicKey` DER from the core.
    private static func encrypt(_ password: String, with der: Data) -> Data? {
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeRSA,
            kSecAttrKeyClass as String: kSecAttrKeyClassPublic,
        ]
        var error: Unmanaged<CFError>?
        guard let key = SecKeyCreateWithData(der as CFData, attributes as CFDictionary, &error) else {
            return nil
        }
        return SecKeyCreateEncryptedData(key, .rsaEncryptionPKCS1, Data(password.utf8) as CFData, &error) as Data?
    }

    /// Empty on failure: the core then reports `parse-entropy`.
    private static func randomBytes(_ count: Int) -> Data {
        var bytes = [UInt8](repeating: 0, count: count)
        guard SecRandomCopyBytes(kSecRandomDefault, count, &bytes) == errSecSuccess else { return Data() }
        return Data(bytes)
    }

    private static func nowMs() -> UInt64 {
        UInt64(Date().timeIntervalSince1970 * 1000)
    }

    private static var language: String {
        Locale.current.language.languageCode?.identifier ?? "en"
    }

    /// e.g. `+01:00`.
    private static var timezone: String {
        let seconds = TimeZone.current.secondsFromGMT()
        let minutes = abs(seconds) / 60
        return String(format: "%@%02d:%02d", seconds < 0 ? "-" : "+", minutes / 60, minutes % 60)
    }
}

/// Returns a 3xx as is, so the core can read its `Location`.
final class NoRedirect: NSObject, URLSessionTaskDelegate, @unchecked Sendable {
    static let shared = NoRedirect()

    func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        willPerformHTTPRedirection response: HTTPURLResponse,
        newRequest request: URLRequest
    ) async -> URLRequest? {
        nil
    }
}

/// `bluelink_constants.json`, written into the bundle by `scripts/ios-build.sh`.
enum BundledConstants {
    /// Empty when missing: the core then reports `parse-constants`.
    static let text: String = Bundle.main.url(forResource: "bluelink_constants", withExtension: "json")
        .flatMap { try? String(contentsOf: $0, encoding: .utf8) } ?? ""
}

/// The in-car repair after `gate9035` / `gate9036`: upstream's current
/// `cci_fingerprint.py`, read by the core, kept until the next build.
enum SignatureRepair {
    private static let source = URL(string:
        "https://raw.githubusercontent.com/Hyundai-Kia-Connect/hyundai_kia_connect_api/master/hyundai_kia_connect_api/cci_fingerprint.py")!

    private struct Stored: Codable {
        /// SHA-256 of the bundled constants it was made for.
        let constants: String
        let override: String
    }

    private static var file: URL {
        FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("fingerprint_override.json")
    }

    /// The stored override, if it was made for this build's constants.
    static func storedOverride(for constants: String) -> String? {
        guard let data = try? Data(contentsOf: file),
              let stored = try? JSONDecoder().decode(Stored.self, from: data),
              stored.constants == digest(constants)
        else { return nil }
        return stored.override
    }

    /// Downloads, reads and stores the override. Returns `ok` or a short code.
    static func update(for constants: String) async -> String {
        do {
            let request = URLRequest(url: source, cachePolicy: .reloadIgnoringLocalCacheData, timeoutInterval: 30)
            let (data, response) = try await URLSession.shared.data(for: request)
            let status = (response as? HTTPURLResponse)?.statusCode ?? 0
            guard status == 200 else { return "http\(status)" }
            let json = try fingerprintOverrideFromUpstream(source: String(decoding: data, as: UTF8.self))
            try FileManager.default.createDirectory(
                at: file.deletingLastPathComponent(),
                withIntermediateDirectories: true
            )
            try JSONEncoder().encode(Stored(constants: digest(constants), override: json))
                .write(to: file, options: .atomic)
            return "ok"
        } catch BridgeError.Invalid(let code) {
            return code
        } catch let error as URLError {
            return "net\(abs(error.errorCode))"
        } catch {
            return "file"
        }
    }

    private static func digest(_ text: String) -> String {
        SHA256.hash(data: Data(text.utf8)).map { String(format: "%02x", $0) }.joined()
    }
}
