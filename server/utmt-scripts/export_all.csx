// Run by dre-server: UndertaleModCli load <data.win> -s export_all.csx
// Env DRE_PROJECT_DIR: empty directory for the UTMT project export (editable assets).
// Env DRE_GENERIC_DIR: directory for the generic JSON dump (every list in the data file).
// Env DRE_SKIP_PROJECT=1: do the generic dump only.
using System;
using System.Collections;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Text.Json;
using UndertaleModLib;
using UndertaleModLib.Models;
using UndertaleModLib.Project;

string projectDir = Environment.GetEnvironmentVariable("DRE_PROJECT_DIR");
string genericDir = Environment.GetEnvironmentVariable("DRE_GENERIC_DIR");
bool skipProject = Environment.GetEnvironmentVariable("DRE_SKIP_PROJECT") == "1";
if (string.IsNullOrEmpty(projectDir) || string.IsNullOrEmpty(genericDir))
    throw new ScriptException("DRE_PROJECT_DIR and DRE_GENERIC_DIR must be set");

void Progress(string msg) => Console.WriteLine($"[dre] {msg}");

// ---- 1. UTMT project export (round-trips through `UndertaleModCli project build`) ----
if (!skipProject)
{
    string mainFile = Path.Join(projectDir, "project.json");
    var project = new ProjectContext(Data, FilePath, FilePath, mainFile, Data.GeneralInfo?.DisplayName?.Content ?? "dre-export");
    int marked = 0;
    foreach (PropertyInfo listProp in Data.AllListProperties)
    {
        if (listProp.GetValue(Data) is not IList list)
            continue;
        foreach (object item in list)
        {
            if (item is IProjectAsset asset && asset.ProjectExportable && project.MarkAssetForExport(asset))
                marked++;
        }
    }
    Progress($"project: exporting {marked} assets");
    project.Export(true);
    Progress("project: done");
}

// ---- 2. Generic dump: every list and singleton in the data file, read-only JSON ----
Directory.CreateDirectory(genericDir);
const int MaxDepth = 4;
var skipLists = new HashSet<string> { "Code", "EmbeddedAudio", "EmbeddedTextures" };
var jsonOpts = new JsonWriterOptions { Indented = false, Encoder = System.Text.Encodings.Web.JavaScriptEncoder.UnsafeRelaxedJsonEscaping };

bool IsPrimitive(Type t) =>
    t.IsPrimitive || t.IsEnum || t == typeof(string) || t == typeof(decimal) || t == typeof(Guid) || t == typeof(DateTime);

string RefName(object o)
{
    if (o is UndertaleNamedResource nr && nr.Name is not null)
        return nr.Name.Content;
    return null;
}

void WriteValue(Utf8JsonWriter w, object v, int depth)
{
    switch (v)
    {
        case null: w.WriteNullValue(); return;
        case UndertaleString s: w.WriteStringValue(s.Content); return;
        case string s: w.WriteStringValue(s); return;
        case bool b: w.WriteBooleanValue(b); return;
        case float f: if (float.IsFinite(f)) w.WriteNumberValue(f); else w.WriteStringValue(f.ToString()); return;
        case double d: if (double.IsFinite(d)) w.WriteNumberValue(d); else w.WriteStringValue(d.ToString()); return;
        case byte[] bytes: w.WriteStringValue($"<{bytes.Length} bytes>"); return;
    }
    Type t = v.GetType();
    if (t.IsEnum) { w.WriteStringValue(v.ToString()); return; }
    if (t.IsPrimitive || t == typeof(decimal)) { w.WriteRawValue(Convert.ToString(v, System.Globalization.CultureInfo.InvariantCulture)); return; }
    if (IsPrimitive(t)) { w.WriteStringValue(v.ToString()); return; }
    if (t.Name.StartsWith("UndertaleResourceById"))
    {
        WriteValue(w, t.GetProperty("Resource")?.GetValue(v), depth);
        return;
    }
    // Named resources below the top level are references: write their name only.
    if (depth > 0 && RefName(v) is string refName)
    {
        w.WriteStartObject(); w.WriteString("$ref", refName); w.WriteString("$kind", t.Name); w.WriteEndObject();
        return;
    }
    if (depth >= MaxDepth) { w.WriteStringValue(v.ToString()); return; }
    if (v is IEnumerable e)
    {
        w.WriteStartArray();
        foreach (object item in e)
            WriteValue(w, item, depth + 1);
        w.WriteEndArray();
        return;
    }
    w.WriteStartObject();
    w.WriteString("$kind", t.Name);
    foreach (PropertyInfo p in t.GetProperties(BindingFlags.Public | BindingFlags.Instance))
    {
        if (p.GetIndexParameters().Length > 0 || !p.CanRead)
            continue;
        // Avoid lazy texture/audio decoding.
        string ptn = p.PropertyType.Name;
        if (ptn == "GMImage" || ptn == "UndertaleEmbeddedTexture" || ptn == "TexData" || ptn == "UndertaleEmbeddedAudio")
            continue;
        object pv;
        try { pv = p.GetValue(v); }
        catch { continue; }
        w.WritePropertyName(p.Name);
        try { WriteValue(w, pv, depth + 1); }
        catch (Exception ex) { w.WriteStringValue($"<error: {ex.GetType().Name}>"); }
    }
    w.WriteEndObject();
}

void DumpTo(string name, object value, bool isList)
{
    string path = Path.Join(genericDir, $"{name}.json");
    using var fs = new FileStream(path, FileMode.Create);
    using var w = new Utf8JsonWriter(fs, jsonOpts);
    if (isList && value is IList list)
    {
        w.WriteStartArray();
        int i = 0;
        foreach (object item in list)
        {
            w.WriteStartObject();
            w.WriteNumber("$index", i++);
            w.WritePropertyName("value");
            WriteValue(w, item, 0);
            w.WriteEndObject();
        }
        w.WriteEndArray();
    }
    else
    {
        WriteValue(w, value, 0);
    }
}

var index = new Dictionary<string, int>();
foreach (string single in new[] { "GeneralInfo", "Options", "Language" })
{
    object v = typeof(UndertaleData).GetProperty(single)?.GetValue(Data);
    if (v is null) continue;
    DumpTo(single, v, false);
    index[single] = -1;
}
foreach (PropertyInfo listProp in Data.AllListProperties)
{
    if (skipLists.Contains(listProp.Name) || listProp.GetValue(Data) is not IList list)
        continue;
    Progress($"generic: {listProp.Name} ({list.Count})");
    DumpTo(listProp.Name, list, true);
    index[listProp.Name] = list.Count;
}
foreach (string listName in skipLists)
{
    if (typeof(UndertaleData).GetProperty(listName)?.GetValue(Data) is IList list)
        index[listName] = list.Count;
}
File.WriteAllText(Path.Join(genericDir, "_index.json"), JsonSerializer.Serialize(index));
Progress("generic: done");
