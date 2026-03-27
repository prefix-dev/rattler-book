"""Margin note block extension for Tufte-style sidenotes."""
import xml.etree.ElementTree as etree
from pymdownx.blocks.block import Block
from pymdownx.blocks import BlocksExtension


class MarginNote(Block):
    """Margin note rendered as a checkbox-toggled aside."""

    NAME = 'margin-note'
    ARGUMENT = False
    OPTIONS = {}

    def on_init(self):
        """Initialize the margin note counter."""
        if 'margin_note_count' not in self.tracker:
            self.tracker['margin_note_count'] = 0

    def on_create(self, parent):
        """Create wrapper with checkbox toggle and aside."""
        self.tracker['margin_note_count'] += 1
        mn_id = "mn-{}".format(self.tracker['margin_note_count'])

        wrapper = etree.SubElement(parent, 'div', {'class': 'margin-wrapper'})
        etree.SubElement(wrapper, 'input', {
            'type': 'checkbox',
            'class': 'margin-toggle',
            'id': mn_id,
        })
        etree.SubElement(wrapper, 'label', {
            'for': mn_id,
            'class': 'margin-toggle-label',
        })
        self.aside = etree.SubElement(wrapper, 'aside', {'class': 'margin-note'})
        return self.aside

    def on_add(self, block):
        """Direct content into the aside element."""
        return self.aside


class MarginNoteExtension(BlocksExtension):
    """Register the margin-note block."""

    def __init__(self, *args, **kwargs):
        self.config = {}
        super().__init__(*args, **kwargs)

    def extendMarkdownBlocks(self, md, block_mgr):
        block_mgr.register(MarginNote, self.getConfigs())


def makeExtension(*args, **kwargs):
    return MarginNoteExtension(*args, **kwargs)
