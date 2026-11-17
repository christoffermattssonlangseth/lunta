#if os(macOS)
import Foundation
import Security

/// The certificates behind https://localhost:47481: a certificate authority made on this Mac
/// (trusted once, with Lunta › Citations in Word & Google Docs… › Trust Certificate) and the
/// localhost certificate it signs, made with the system's openssl.
///
/// macOS accepts server certificates for at most 825 days, so the localhost certificate is
/// renewed a month before it runs out. The authority may only vouch for localhost and 127.0.0.1
/// (name constraints), so its key is kept for the renewals; then nothing has to be trusted again.
/// Authorities made before that (key deleted) are replaced when the certificate runs out, and
/// the new one must be trusted once more.
public enum CitationCertificates {
    public enum Outcome: Equatable, Sendable {
        /// The certificate is good for at least another month.
        case current
        /// A new localhost certificate from the same authority: still trusted.
        case renewed
        /// A new authority: it must be trusted (Trust Certificate…) before the add-ins connect.
        case newAuthority
        case failed(String)
    }

    static let renewBefore: TimeInterval = 30 * 86400
    static let p12Password = "margins"

    /// Makes or renews the certificates in `dir` as needed.
    @discardableResult
    public static func ensure(in dir: URL, now: Date = Date()) -> Outcome {
        let fm = FileManager.default
        let p12 = dir.appendingPathComponent("server.p12"), pem = dir.appendingPathComponent("server.pem")
        let caPem = dir.appendingPathComponent("ca.pem"), caKey = dir.appendingPathComponent("ca.key")
        let left = { (url: URL) in expiry(of: url).map { $0.timeIntervalSince(now) } ?? 0 }
        if fm.fileExists(atPath: p12.path), left(pem) > renewBefore, left(caPem) > renewBefore {
            return .current
        }
        try? fm.createDirectory(at: dir, withIntermediateDirectories: true)
        var newAuthority = false
        // a new authority when there is no key to sign with, or the next certificate would outlive it
        if !fm.fileExists(atPath: caKey.path) || left(caPem) < 830 * 86400 {
            let ext = """
            basicConstraints=critical,CA:TRUE,pathlen:0
            keyUsage=critical,keyCertSign,cRLSign
            nameConstraints=critical,permitted;DNS:localhost,permitted;IP:127.0.0.1/255.255.255.255
            subjectKeyIdentifier=hash
            """
            do { try ext.write(to: dir.appendingPathComponent("ca.ext"), atomically: true, encoding: .utf8) } catch { return .failed("\(error)") }
            if let e = openssl(dir, ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "3650", "-keyout", "ca.key", "-out", "ca.pem",
                                     "-subj", "/CN=Lunta Local CA (this Mac only, localhost)", "-extensions", "v3_ca_lunta",
                                     "-config", configFile(dir, section: "v3_ca_lunta", ext: ext).path]) {
                return .failed(e)
            }
            newAuthority = true
        }
        let ext = """
        basicConstraints=CA:FALSE
        keyUsage=critical,digitalSignature,keyEncipherment
        extendedKeyUsage=serverAuth
        subjectAltName=DNS:localhost,IP:127.0.0.1
        """
        do { try ext.write(to: dir.appendingPathComponent("server.ext"), atomically: true, encoding: .utf8) } catch { return .failed("\(error)") }
        // made beside the old files and moved over them only when complete
        let steps: [[String]] = [
            ["req", "-newkey", "rsa:2048", "-nodes", "-keyout", "server.key.new", "-out", "server.csr", "-subj", "/CN=localhost"],
            ["x509", "-req", "-in", "server.csr", "-CA", "ca.pem", "-CAkey", "ca.key", "-CAcreateserial",
             "-out", "server.pem.new", "-days", "820", "-sha256", "-extfile", "server.ext"],
            ["pkcs12", "-export", "-in", "server.pem.new", "-inkey", "server.key.new", "-out", "server.p12.new", "-passout", "pass:\(p12Password)"],
        ]
        for args in steps {
            if let e = openssl(dir, args) { return .failed(e) }
        }
        for name in ["server.key", "server.pem", "server.p12"] {
            let to = dir.appendingPathComponent(name)
            _ = try? fm.replaceItemAt(to, withItemAt: dir.appendingPathComponent(name + ".new"))
            if !fm.fileExists(atPath: to.path) { try? fm.moveItem(at: dir.appendingPathComponent(name + ".new"), to: to) }
        }
        for name in ["ca.key", "server.key", "server.p12"] {
            try? fm.setAttributes([.posixPermissions: 0o600], ofItemAtPath: dir.appendingPathComponent(name).path)
        }
        return newAuthority ? .newAuthority : .renewed
    }

    /// The identity the server presents.
    public static func identity(in dir: URL) -> SecIdentity? {
        guard let data = try? Data(contentsOf: dir.appendingPathComponent("server.p12")) else { return nil }
        var items: CFArray?
        let status = SecPKCS12Import(data as CFData, [kSecImportExportPassphrase as String: p12Password] as CFDictionary, &items)
        guard status == errSecSuccess, let arr = items as? [[String: Any]], let first = arr.first,
              let id = first[kSecImportItemIdentity as String] else { return nil }
        return (id as! SecIdentity)
    }

    public static func certificate(_ pem: URL) -> SecCertificate? {
        guard let text = try? String(contentsOf: pem, encoding: .utf8) else { return nil }
        let b64 = text.components(separatedBy: "\n").filter { !$0.hasPrefix("-----") }.joined()
        guard let der = Data(base64Encoded: b64) else { return nil }
        return SecCertificateCreateWithData(nil, der as CFData)
    }

    /// When a PEM certificate runs out.
    public static func expiry(of pem: URL) -> Date? {
        guard let cert = certificate(pem),
              let values = SecCertificateCopyValues(cert, [kSecOIDX509V1ValidityNotAfter] as CFArray, nil) as? [String: Any],
              let entry = values[kSecOIDX509V1ValidityNotAfter as String] as? [String: Any],
              let n = entry[kSecPropertyKeyValue as String] as? NSNumber else { return nil }
        return Date(timeIntervalSinceReferenceDate: n.doubleValue)
    }

    /// Whether the localhost certificate in `dir` is valid when `anchors` are the only trusted
    /// roots (nil: the roots this Mac trusts, as Word and the browsers see them).
    public static func evaluate(in dir: URL, anchors: [SecCertificate]? = nil, host: String = "localhost") -> Bool {
        guard let leaf = certificate(dir.appendingPathComponent("server.pem")) else { return false }
        var trust: SecTrust?
        guard SecTrustCreateWithCertificates(leaf, SecPolicyCreateSSL(true, host as CFString), &trust) == errSecSuccess, let trust else { return false }
        if let anchors {
            SecTrustSetAnchorCertificates(trust, anchors as CFArray)
            SecTrustSetAnchorCertificatesOnly(trust, true)
        }
        return SecTrustEvaluateWithError(trust, nil)
    }

    private static func configFile(_ dir: URL, section: String, ext: String) -> URL {
        let url = dir.appendingPathComponent("ca.cnf")
        let cnf = "[req]\ndistinguished_name=dn\n[dn]\n[\(section)]\n\(ext)\n"
        try? cnf.write(to: url, atomically: true, encoding: .utf8)
        return url
    }

    /// Runs /usr/bin/openssl in `dir`; the error, or nil.
    private static func openssl(_ dir: URL, _ args: [String]) -> String? {
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/openssl")
        p.arguments = args
        p.currentDirectoryURL = dir
        let err = Pipe()
        p.standardOutput = FileHandle.nullDevice
        p.standardError = err
        do { try p.run() } catch { return "openssl: \(error.localizedDescription)" }
        let msg = err.fileHandleForReading.readDataToEndOfFile()
        p.waitUntilExit()
        return p.terminationStatus == 0 ? nil : "openssl \(args.first ?? ""): " + String(decoding: msg, as: UTF8.self).trimmingCharacters(in: .whitespacesAndNewlines)
    }
}
#endif
