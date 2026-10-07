// Generate a binary YMAP from our owned XML fixture for extractor round-trip tests.
using CodeWalker.GameFiles;
using System.Xml;

if (args.Length != 2) return 2;
try {
    string input = Path.GetFullPath(args[0]);
    string output = Path.GetFullPath(args[1]);
    if (File.Exists(output)) throw new IOException("Fixture output already exists.");
    if (new FileInfo(input).Length > 1024 * 1024) throw new IOException("Fixture exceeds 1 MiB.");
    var settings = new XmlReaderSettings { DtdProcessing = DtdProcessing.Prohibit, XmlResolver = null };
    using var reader = XmlReader.Create(input, settings);
    var document = new XmlDocument { XmlResolver = null };
    document.Load(reader);
    if (document.DocumentElement?.Name != "CMapData") throw new InvalidDataException("Expected CMapData fixture.");
    byte[] data = XmlMeta.GetRSCData(document);
    if (data == null || data.Length < 16) throw new InvalidDataException("Fixture did not encode.");
    using var file = new FileStream(output, FileMode.CreateNew);
    file.Write(data);
    return 0;
} catch (Exception error) {
    Console.Error.WriteLine(error.Message);
    return 1;
}
