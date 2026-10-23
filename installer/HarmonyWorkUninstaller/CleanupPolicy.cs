using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Threading;

namespace HarmonyWorkUninstaller;

public static class CleanupPolicy
{
    public const string AppId = "com.harmony.xiaoheng-workbuddy";

    public static IReadOnlyList<string> DataRoots(string roaming, string local)
    {
        return new[] {
            Path.GetFullPath(Path.Combine(roaming, AppId)),
            Path.GetFullPath(Path.Combine(local, AppId)),
            Path.GetFullPath(Path.Combine(local, "Harmony", "WorkBuddy")),
        };
    }

    public static IReadOnlyList<string> ProtectedPaths(string roaming, string local, string home)
    {
        var paths = new HashSet<string>(StringComparer.OrdinalIgnoreCase) {
            Path.GetFullPath(Path.Combine(local, "Harmony", "WorkBuddy", "runtime")),
            Path.GetFullPath(Path.Combine(home, "harmony")),
        };
        var appDataRoots = DataRoots(roaming, local).Take(2).ToArray();
        var foundConfig = false;
        var uncertainWorkspace = false;
        foreach (var root in appDataRoots) {
            var configPath = Path.Combine(root, "config.json");
            if (!File.Exists(configPath)) continue;
            foundConfig = true;
            try {
                using var json = JsonDocument.Parse(File.ReadAllText(configPath));
                if (json.RootElement.TryGetProperty("workspace_root", out var value) &&
                    value.ValueKind == JsonValueKind.String &&
                    !string.IsNullOrWhiteSpace(value.GetString())) {
                    paths.Add(Path.GetFullPath(value.GetString()!));
                }
            } catch (IOException) {
                uncertainWorkspace = true;
            } catch (JsonException) {
                uncertainWorkspace = true;
            } catch (ArgumentException) {
                uncertainWorkspace = true;
            } catch (UnauthorizedAccessException) {
                uncertainWorkspace = true;
            }
        }
        if (!foundConfig || uncertainWorkspace) {
            // Without a trustworthy workspace setting, keep every app-owned root;
            // the chosen workspace may be nested under the legacy data root too.
            foreach (var root in DataRoots(roaming, local).Where(Directory.Exists)) paths.Add(root);
        }
        return paths.ToArray();
    }

    public static bool IsProtected(string path, IEnumerable<string> protectedPaths)
    {
        var normalized = Normalize(path);
        return protectedPaths.Any(item => {
            var protectedPath = Normalize(item);
            return normalized.Equals(protectedPath, StringComparison.OrdinalIgnoreCase) ||
                protectedPath.StartsWith(normalized + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
        });
    }

    public static bool IsWithin(string path, string root)
    {
        var normalized = Normalize(path);
        var normalizedRoot = Normalize(root);
        return normalized.Equals(normalizedRoot, StringComparison.OrdinalIgnoreCase) ||
            normalized.StartsWith(normalizedRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase);
    }

    public static int CleanOwnedRoot(string root, IReadOnlyList<string> protectedPaths,
        List<string> retained, List<string> errors, CancellationToken cancellationToken)
    {
        var normalizedRoot = Normalize(root);
        if (!Directory.Exists(normalizedRoot)) return 0;
        var removed = 0;
        CleanDirectory(normalizedRoot, normalizedRoot, protectedPaths, retained, errors, ref removed, cancellationToken);
        return removed;
    }

    private static void CleanDirectory(string path, string root, IReadOnlyList<string> protectedPaths,
        List<string> retained, List<string> errors, ref int removed, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (!IsWithin(path, root)) throw new InvalidOperationException("Cleanup path escaped its app-owned root.");
        if (protectedPaths.Any(item => Normalize(item).Equals(Normalize(path), StringComparison.OrdinalIgnoreCase))) {
            retained.Add(path);
            return;
        }
        try {
            var info = new DirectoryInfo(path);
            if ((info.Attributes & FileAttributes.ReparsePoint) != 0) {
                retained.Add(path);
                return;
            }
            foreach (var file in info.GetFiles()) {
                cancellationToken.ThrowIfCancellationRequested();
                try {
                    if ((file.Attributes & FileAttributes.ReparsePoint) != 0) {
                        retained.Add(file.FullName);
                    } else {
                        file.Delete();
                        removed++;
                    }
                } catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) {
                    errors.Add($"{file.FullName}: {ex.Message}");
                }
            }
            foreach (var child in info.GetDirectories()) {
                CleanDirectory(child.FullName, root, protectedPaths, retained, errors, ref removed, cancellationToken);
            }
            if (!Directory.EnumerateFileSystemEntries(path).Any()) {
                info.Delete();
                removed++;
            }
        } catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) {
            errors.Add($"{path}: {ex.Message}");
        }
    }

    private static string Normalize(string path) => Path.GetFullPath(path)
        .TrimEnd(Path.DirectorySeparatorChar, Path.AltDirectorySeparatorChar);
}
