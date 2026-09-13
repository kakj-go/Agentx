import { BoundaryNode } from '../nodes/boundary-node'
import { ExitNode } from '../nodes/exit-node'
import { ManifestNode } from '../nodes/manifest-node'
import { AnnotationNode, GroupNode, IterationChipNode, IterationEndNode, LoopContainerNode } from './editor-overlays'

const canvasPackages = [
  {
    packageId: 'agentx/core',
    packageVersion: '1.0.0',
    renderers: {
      boundary: BoundaryNode,
      exit: ExitNode,
      manifest: ManifestNode,
      'loop-container': LoopContainerNode,
      'iteration-chip': IterationChipNode,
      'iteration-end': IterationEndNode,
    },
  },
  {
    packageId: 'agentx/studio',
    packageVersion: '1.0.0',
    renderers: { annotation: AnnotationNode, group: GroupNode },
  },
]

export const CANVAS_NODE_TYPES = Object.assign({}, ...canvasPackages.map((item) => item.renderers))
