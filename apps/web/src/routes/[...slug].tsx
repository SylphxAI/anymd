import { useParams } from '@solidjs/router';
import { Doc } from '@/lib/docs';

export default function Article() {
  const params = useParams();
  return <Doc name={params.slug ?? ''} />;
}
