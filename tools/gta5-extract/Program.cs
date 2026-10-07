using CodeWalker.GameFiles;
using System.Xml.Linq;

if (args.Length != 2) {
    Console.Error.WriteLine("Usage: gta5-extract <model.yft|model.ydd|model.ydr|textures.ytd|map.ymap|types.ytyp> <new-output-directory>");
    return 2;
}
try {
    string input = Path.GetFullPath(args[0]);
    string output = Path.GetFullPath(args[1]);
    if (Directory.Exists(output)) throw new IOException("Output directory already exists; use a new directory.");
    if (new FileInfo(input).Length > 64 * 1024 * 1024) throw new IOException("Input exceeds 64 MiB.");
    byte[] data = File.ReadAllBytes(input);
    if (data.Length < 16 || System.Text.Encoding.ASCII.GetString(data, 0, 4) != "RSC7")
        throw new InvalidDataException("Expected an unencrypted GTA V legacy RSC7 resource.");
    // Bound the declared decompressed allocation before entering the legacy decoder.
    long resourceSize(uint flags) {
        (int Shift, uint Mask, int Weight)[] fields = [(27,1,1),(26,1,2),(25,1,4),(24,1,8),
            (17,127,16),(11,63,32),(7,15,64),(5,3,128),(4,1,256)];
        long pages = fields.Sum(f => (long)((flags >> f.Shift) & f.Mask) * f.Weight);
        return pages * (512L << (int)(flags & 15));
    }
    long allocation = resourceSize(BitConverter.ToUInt32(data,8)) + resourceSize(BitConverter.ToUInt32(data,12));
    if (allocation == 0 || allocation > 256 * 1024 * 1024)
        throw new InvalidDataException("Declared resource memory exceeds 256 MiB or is empty.");
    Func<string, string> export;
    switch (Path.GetExtension(input).ToLowerInvariant()) {
        case ".yft": var yft = new YftFile(); yft.Load(data); export = dir => YftXml.GetXml(yft, dir); break;
        case ".ydd": var ydd = new YddFile(); ydd.Load(data); export = dir => YddXml.GetXml(ydd, dir); break;
        case ".ydr": var ydr = new YdrFile(); ydr.Load(data); export = dir => YdrXml.GetXml(ydr, dir); break;
        case ".ytd": var ytd = new YtdFile(); ytd.Load(data); export = dir => YtdXml.GetXml(ytd, dir); break;
        case ".ymap": var ymap = new YmapFile(); ymap.Load(data); export = dir => MetaXml.GetXml(ymap, out _); break;
        case ".ytyp": var ytyp = new YtypFile(); ytyp.Load(data); export = dir => MetaXml.GetXml(ytyp, out _); break;
        default: throw new InvalidDataException("Unsupported extension.");
    }
    var preview = XDocument.Parse(export(""));
    foreach (var texture in preview.Root!.DescendantsAndSelf("TextureDictionary").Elements("Item")) {
        string name = texture.Element("Name")?.Value ?? "";
        if (name.Length == 0 || name.Length > 128 || name.IndexOfAny(['/', '\\', ':']) >= 0 || name is "." or ".." || name.Any(c => c < 32 || "<>\"|?*".Contains(c)))
            throw new InvalidDataException("Unsafe embedded texture name.");
    }
    Directory.CreateDirectory(output);
    string xml = export(output);
    File.WriteAllText(Path.Combine(output, Path.GetFileName(input) + ".xml"), xml);
    Console.WriteLine("Exported CodeWalker XML and embedded DDS textures to " + output);
    return 0;
} catch (Exception error) {
    Console.Error.WriteLine("Extraction failed: " + error.Message);
    return 1;
}
