// What Math prints: the formula, centred near the top of one A4 page.

import 'package:material_ui/material_ui.dart';

import '../core/print_preview.dart';
import 'math_view.dart';

/// The print job of a formula at [fontPt] points.
PrintJob mathPrintJob(Map<String, dynamic> tree, {String? title, double fontPt = 24, bool landscape = false}) {
  return PrintJob(
    width: landscape ? a4Height : a4Width,
    height: landscape ? a4Width : a4Height,
    margin: 56.69,
    units: [
      PrintUnit((page, pages, first) => Padding(
            padding: const EdgeInsets.only(top: 24),
            child: Center(
              child: FittedBox(
                // Wide formulas shrink to the text width.
                fit: BoxFit.scaleDown,
                child: MathView(tree: tree, fontSize: fontPt, color: const Color(0xFF000000)),
              ),
            ),
          )),
    ],
    paper: 'a4',
    title: title,
  );
}
