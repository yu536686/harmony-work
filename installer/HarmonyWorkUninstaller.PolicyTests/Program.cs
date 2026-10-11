using HarmonyWorkUninstaller;

var root = Path.Combine(Path.GetTempPath(), "harmony-uninstall-policy-" + Guid.NewGuid().ToString("N"));
var roaming = Path.Combine(root, "roaming");
var local = Path.Combine(root, "local");
var home = Path.Combine(root, "home");
try {
    Directory.CreateDirectory(Path.Combine(roaming, CleanupPolicy.AppId));
    var nestedWorkspace = Path.Combine(roaming, CleanupPolicy.AppId, "projects", "my-work");
    File.WriteAllText(Path.Combine(roaming, CleanupPolicy.AppId, "config.json"),
        System.Text.Json.JsonSerializer.Serialize(new { workspace_root = nestedWorkspace }));
    var roots = CleanupPolicy.DataRoots(roaming, local);
    var protectedPaths = CleanupPolicy.ProtectedPaths(roaming, local, home);
    Assert(roots.Count == 3, "Only the three known app-owned roots may be scanned");
    Assert(CleanupPolicy.IsProtected(nestedWorkspace, protectedPaths), "Selected workspace must be protected");
    Assert(CleanupPolicy.IsProtected(Path.Combine(roaming, CleanupPolicy.AppId, "projects"), protectedPaths),
        "Workspace ancestor must be protected");
    Assert(!CleanupPolicy.IsProtected(Path.Combine(roaming, CleanupPolicy.AppId, "skills"), protectedPaths),
        "Installed skills may be removed");
    Assert(CleanupPolicy.IsProtected(Path.Combine(local, "Harmony", "WorkBuddy", "runtime"), protectedPaths),
        "Bundled runtime must be protected");
    Assert(!CleanupPolicy.IsWithin(Path.Combine(home, "Documents", "photo.jpg"), roots[0]),
        "User documents must be outside cleanup scope");
    Assert(!CleanupPolicy.IsWithin(roots[0] + "-other", roots[0]), "Sibling directories must stay outside scope");
    Directory.CreateDirectory(nestedWorkspace);
    File.WriteAllText(Path.Combine(nestedWorkspace, "user.pdf"), "keep");
    Directory.CreateDirectory(Path.Combine(roaming, CleanupPolicy.AppId, "skills"));
    File.WriteAllText(Path.Combine(roaming, CleanupPolicy.AppId, "skills", "installed.txt"), "remove");
    var retained = new List<string>();
    var errors = new List<string>();
    CleanupPolicy.CleanOwnedRoot(roots[0], protectedPaths, retained, errors, CancellationToken.None);
    Assert(errors.Count == 0, "Cleanup must not fail");
    Assert(File.Exists(Path.Combine(nestedWorkspace, "user.pdf")), "Actual workspace files must survive cleanup");
    Assert(!Directory.Exists(Path.Combine(roaming, CleanupPolicy.AppId, "skills")), "Installed skills must be removed");
    Assert(retained.Contains(nestedWorkspace), "Cleanup report must name the preserved workspace");
    Assert(CleanupPolicy.IsProtected(roots[0], CleanupPolicy.ProtectedPaths(roaming, local, home)),
        "Missing settings must fail closed while app data remains");
    var legacyRoot = roots[2];
    Directory.CreateDirectory(legacyRoot);
    var unknownWorkspaceFile = Path.Combine(legacyRoot, "unknown-workspace", "keep.pdf");
    Directory.CreateDirectory(Path.GetDirectoryName(unknownWorkspaceFile)!);
    File.WriteAllText(unknownWorkspaceFile, "keep");
    CleanupPolicy.CleanOwnedRoot(legacyRoot, CleanupPolicy.ProtectedPaths(roaming, local, home),
        retained, errors, CancellationToken.None);
    Assert(File.Exists(unknownWorkspaceFile), "Missing settings must retain unknown workspaces in the legacy root");
    File.WriteAllText(Path.Combine(roaming, CleanupPolicy.AppId, "config.json"), "{");
    Assert(CleanupPolicy.IsProtected(roots[0], CleanupPolicy.ProtectedPaths(roaming, local, home)),
        "Invalid settings must fail closed");
    Console.WriteLine("Uninstall cleanup policy: all assertions passed.");
} finally {
    if (Directory.Exists(root)) Directory.Delete(root, recursive: true);
}

static void Assert(bool condition, string message)
{
    if (!condition) throw new Exception(message);
}
